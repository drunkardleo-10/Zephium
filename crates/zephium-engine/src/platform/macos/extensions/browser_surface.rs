//! Main-thread WKWebExtension window/tab delegate graph.
//!
//! The ordinary graph mirrors Shell-owned logical identities but never creates
//! a `WKWebView`. A normal tab callback returns a native view only when the
//! Shell marks the tab resident and the engine already owns that exact
//! physical view. Same-principal extension documents use the separate bounded
//! extension-UI host and never enter the ordinary navigation model.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::ptr::NonNull;
use std::rc::Rc;
#[cfg(feature = "native-web-extension-probes")]
use std::sync::atomic::{AtomicUsize, Ordering};
#[cfg(feature = "native-web-extension-probes")]
use std::sync::Arc;
use std::sync::OnceLock;

use objc2::rc::{Retained, Weak};
use objc2::runtime::{AnyObject, NSObject, ProtocolObject};
use objc2::{define_class, msg_send, DefinedClass, MainThreadOnly};
use objc2_app_kit::NSView;
#[cfg(feature = "native-extension-qa-inspector")]
use objc2_foundation::NSLocalizedFailureReasonErrorKey;
use objc2_foundation::{
    MainThreadMarker, NSArray, NSDate, NSError, NSNotFound, NSObjectProtocol, NSSet, NSString,
    NSURLErrorFailingURLErrorKey, NSUTF8StringEncoding, NSUnderlyingErrorKey, NSURL,
};
use objc2_web_kit::{
    WKWebExtensionAction, WKWebExtensionContext, WKWebExtensionContextErrorDomain,
    WKWebExtensionController, WKWebExtensionControllerDelegate, WKWebExtensionMatchPattern,
    WKWebExtensionMessagePort, WKWebExtensionPermission, WKWebExtensionTab,
    WKWebExtensionTabChangedProperties, WKWebExtensionTabConfiguration, WKWebExtensionWindow,
    WKWebView,
};
use zephium_core::extensions::{
    ExtensionActionRejection, ExtensionActionRequest, ExtensionActionRequestId,
    ExtensionBrowserRequestAction, ExtensionBrowserRequestRejection, ExtensionBrowserSurface,
    ExtensionBrowserSurfaceGeneration, ExtensionCompatibilityBrokerRequestId,
    ExtensionCompatibilityBrokerSettlement, ExtensionCompatibilityBrokerWitness,
    EXTENSION_COMPATIBILITY_BROKER_APPLICATION_ID, MAX_EXTENSION_BROWSER_REQUEST_URL_BYTES,
};
use zephium_core::ids::{ItemId, ProfileId, WindowId};

pub(super) type NativeExtensionTab = Retained<ProtocolObject<dyn WKWebExtensionTab>>;

use super::action_popup::{ActionPopupBroker, ActionPopupPreparation};
use super::browser_request_broker::{
    BrowserRequestBroker, BrowserRequestPool, BrowserRequestSettlementOutcome,
};
use super::compatibility_broker::{
    CompatibilityBroker, CompatibilityBrokerPool, CompatibilityBrokerSettlementOutcome,
};
use super::extension_page::ExtensionPageBroker;
use super::identity_broker::{IdentityBroker, IdentityRequestId};
use super::native_messaging::{
    NativeHostProcessPool, PublisherNativeMessagingAuthorization, PublisherNativeMessagingBroker,
    PublisherNativeMessagingRequestId,
};
use super::offscreen_broker::{OffscreenBroker, OffscreenSessionId};
use super::runtime_grant_broker::{
    RuntimeGrantRequestBroker, RuntimeGrantRequestPool, RuntimeGrantSettlementOutcome,
};

#[cfg(feature = "native-web-extension-probes")]
pub(super) type ProbeBrowserSurfaceIdentity = (
    Retained<ProtocolObject<dyn WKWebExtensionWindow>>,
    Retained<ProtocolObject<dyn WKWebExtensionTab>>,
);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum BrowserSurfaceError {
    MainThreadRequired,
    ProfileMismatch,
    StaleGeneration,
    PrivacyClassChanged,
    IntegrityFailed,
}

/// Bounded, telemetry-free counters for native requests that cannot be
/// represented by Zephium's logical-tab residency model.
#[cfg(feature = "native-web-extension-probes")]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct BrowserSurfaceDiagnostics {
    discarded_tab_webview_refusals: u64,
}

#[cfg(feature = "native-web-extension-probes")]
impl BrowserSurfaceDiagnostics {
    pub(crate) const fn discarded_tab_webview_refusals(self) -> u64 {
        self.discarded_tab_webview_refusals
    }
}

fn resolve_resident_webview(
    resident: bool,
    broker: &BrowserRequestBroker,
    load: impl FnOnce() -> Option<Retained<WKWebView>>,
) -> Option<Retained<WKWebView>> {
    if !resident {
        broker.record_discarded_tab_webview_refusal();
        return None;
    }
    load()
}

struct BrowserTabIvars {
    // ItemId is 16-byte aligned. Keep it behind a pointer-sized field because
    // objc2's dynamic class registrar supports ivar alignments only through 8.
    id: Box<ItemId>,
    window: RefCell<Option<Weak<BrowserWindow>>>,
    webview: RefCell<Option<Weak<WKWebView>>>,
    title: RefCell<Retained<NSString>>,
    url: RefCell<Option<Retained<NSURL>>>,
    attempted_url: RefCell<Option<Retained<NSURL>>>,
    index: Cell<usize>,
    selected: Cell<bool>,
    resident: Cell<bool>,
    loading: Cell<bool>,
    pinned: Cell<bool>,
    broker: Rc<BrowserRequestBroker>,
    guest: RefCell<
        Option<(
            Retained<WKWebExtensionContext>,
            std::rc::Weak<ExtensionPageBroker>,
        )>,
    >,
    #[cfg(feature = "native-web-extension-probes")]
    lifecycle_drops: Arc<AtomicUsize>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "ZephiumExtensionBrowserTab"]
    #[ivars = BrowserTabIvars]
    struct BrowserTab;

    unsafe impl NSObjectProtocol for BrowserTab {}

    unsafe impl WKWebExtensionTab for BrowserTab {
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
            context: &WKWebExtensionContext,
        ) -> Option<Retained<WKWebView>> {
            if let Some((_, broker)) = self.ivars().guest.borrow().as_ref() {
                broker
                    .upgrade()
                    .and_then(|broker| broker.view(self.id(), context))
            } else {
                resolve_resident_webview(self.ivars().resident.get(), &self.ivars().broker, || {
                    self.ivars().webview.borrow().as_ref().and_then(Weak::load)
                })
            }
        }

        #[unsafe(method_id(titleForWebExtensionContext:))]
        fn title_for_context(
            &self,
            _context: &WKWebExtensionContext,
        ) -> Option<Retained<NSString>> {
            Some(self.ivars().title.borrow().clone())
        }

        #[unsafe(method_id(urlForWebExtensionContext:))]
        fn url_for_context(&self, context: &WKWebExtensionContext) -> Option<Retained<NSURL>> {
            if let Some((_, broker)) = self.ivars().guest.borrow().as_ref() {
                broker
                    .upgrade()
                    .and_then(|broker| broker.view(self.id(), context))
                    .and_then(|view| unsafe { view.URL() })
            } else {
                self.ivars()
                    .attempted_url
                    .borrow()
                    .as_ref()
                    .cloned()
                    .or_else(|| self.ivars().url.borrow().clone())
            }
        }

        #[unsafe(method(isLoadingCompleteForWebExtensionContext:))]
        fn is_loading_complete_for_context(&self, _context: &WKWebExtensionContext) -> bool {
            !self.ivars().loading.get()
        }

        #[unsafe(method(isPinnedForWebExtensionContext:))]
        fn is_pinned_for_context(&self, _context: &WKWebExtensionContext) -> bool {
            self.ivars().pinned.get()
        }

        #[unsafe(method(indexInWindowForWebExtensionContext:))]
        fn index_in_window(&self, _context: &WKWebExtensionContext) -> usize {
            self.ivars().index.get()
        }

        #[unsafe(method(isSelectedForWebExtensionContext:))]
        fn is_selected(&self, _context: &WKWebExtensionContext) -> bool {
            self.ivars().selected.get()
        }

        #[unsafe(method(loadURL:forWebExtensionContext:completionHandler:))]
        fn load_url(
            &self,
            url: &NSURL,
            context: &WKWebExtensionContext,
            completion: &block2::DynBlock<dyn Fn(*mut NSError)>,
        ) {
            if !self.accepts(context) {
                self.ivars()
                    .broker
                    .reject_unit(completion, ExtensionBrowserRequestRejection::InvalidContext);
                return;
            }
            if let Some((_, broker)) = self.ivars().guest.borrow().as_ref() {
                if let Some(broker) = broker.upgrade() {
                    broker.load(self.id(), context, url, completion);
                } else {
                    self.ivars()
                        .broker
                        .reject_unit(completion, ExtensionBrowserRequestRejection::InvalidContext);
                }
                return;
            }
            let Ok(url) = request_url(url) else {
                self.ivars()
                    .broker
                    .reject_unit(completion, ExtensionBrowserRequestRejection::InvalidRequest);
                return;
            };
            self.ivars().broker.begin_unit(
                ExtensionBrowserRequestAction::LoadTabUrl {
                    tab: self.id(),
                    url,
                },
                completion,
            );
        }

        #[unsafe(method(activateForWebExtensionContext:completionHandler:))]
        fn activate(
            &self,
            context: &WKWebExtensionContext,
            completion: &block2::DynBlock<dyn Fn(*mut NSError)>,
        ) {
            self.begin_activate(context, completion);
        }

        #[unsafe(method(setSelected:forWebExtensionContext:completionHandler:))]
        fn set_selected(
            &self,
            selected: bool,
            context: &WKWebExtensionContext,
            completion: &block2::DynBlock<dyn Fn(*mut NSError)>,
        ) {
            // Zephium currently has one selected tab per window. Within that
            // product model, selecting a tab means activating it; deselecting
            // the active tab without a replacement has no truthful mapping.
            if selected {
                self.begin_activate(context, completion);
            } else {
                self.reject_unsupported(context, completion);
            }
        }

        #[unsafe(method(closeForWebExtensionContext:completionHandler:))]
        fn close(
            &self,
            context: &WKWebExtensionContext,
            completion: &block2::DynBlock<dyn Fn(*mut NSError)>,
        ) {
            if !self.accepts(context) {
                self.ivars()
                    .broker
                    .reject_unit(completion, ExtensionBrowserRequestRejection::InvalidContext);
                return;
            }
            if extension_tab_trace_enabled() {
                eprintln!(
                    "extension-tab-trace: native-close-request tab={}",
                    self.id()
                );
            }
            self.ivars().broker.begin_unit(
                ExtensionBrowserRequestAction::CloseTab { tab: self.id() },
                completion,
            );
        }

        // WebKit otherwise performs these mutations directly on the WKWebView.
        // Implement them as explicit refusals until their Shell commands and
        // permission semantics are represented by typed core requests.
        #[unsafe(method(reloadFromOrigin:forWebExtensionContext:completionHandler:))]
        fn reload(
            &self,
            from_origin: bool,
            context: &WKWebExtensionContext,
            completion: &block2::DynBlock<dyn Fn(*mut NSError)>,
        ) {
            if from_origin {
                // `Engine::reload` does not represent cache bypass. Calling it
                // here would claim stronger semantics than Zephium applied.
                self.reject_unsupported(context, completion);
                return;
            }
            self.begin_unit_action(
                context,
                completion,
                ExtensionBrowserRequestAction::ReloadTab { tab: self.id() },
            );
        }

        #[unsafe(method(goBackForWebExtensionContext:completionHandler:))]
        fn go_back(
            &self,
            context: &WKWebExtensionContext,
            completion: &block2::DynBlock<dyn Fn(*mut NSError)>,
        ) {
            self.begin_unit_action(
                context,
                completion,
                ExtensionBrowserRequestAction::GoBack { tab: self.id() },
            );
        }

        #[unsafe(method(goForwardForWebExtensionContext:completionHandler:))]
        fn go_forward(
            &self,
            context: &WKWebExtensionContext,
            completion: &block2::DynBlock<dyn Fn(*mut NSError)>,
        ) {
            self.begin_unit_action(
                context,
                completion,
                ExtensionBrowserRequestAction::GoForward { tab: self.id() },
            );
        }

        #[unsafe(method(setParentTab:forWebExtensionContext:completionHandler:))]
        fn set_parent_tab(
            &self,
            _parent: Option<&ProtocolObject<dyn WKWebExtensionTab>>,
            context: &WKWebExtensionContext,
            completion: &block2::DynBlock<dyn Fn(*mut NSError)>,
        ) {
            self.reject_unsupported(context, completion);
        }

        #[unsafe(method(setPinned:forWebExtensionContext:completionHandler:))]
        fn set_pinned(
            &self,
            _pinned: bool,
            context: &WKWebExtensionContext,
            completion: &block2::DynBlock<dyn Fn(*mut NSError)>,
        ) {
            self.reject_unsupported(context, completion);
        }

        #[unsafe(method(setReaderModeActive:forWebExtensionContext:completionHandler:))]
        fn set_reader_mode(
            &self,
            _active: bool,
            context: &WKWebExtensionContext,
            completion: &block2::DynBlock<dyn Fn(*mut NSError)>,
        ) {
            self.reject_unsupported(context, completion);
        }

        #[unsafe(method(setMuted:forWebExtensionContext:completionHandler:))]
        fn set_muted(
            &self,
            _muted: bool,
            context: &WKWebExtensionContext,
            completion: &block2::DynBlock<dyn Fn(*mut NSError)>,
        ) {
            self.reject_unsupported(context, completion);
        }

        #[unsafe(method(setZoomFactor:forWebExtensionContext:completionHandler:))]
        fn set_zoom_factor(
            &self,
            _zoom: f64,
            context: &WKWebExtensionContext,
            completion: &block2::DynBlock<dyn Fn(*mut NSError)>,
        ) {
            self.reject_unsupported(context, completion);
        }

        #[unsafe(method(shouldGrantPermissionsOnUserGestureForWebExtensionContext:))]
        fn should_grant_permissions(&self, _context: &WKWebExtensionContext) -> bool {
            false
        }

        #[unsafe(method(shouldBypassPermissionsForWebExtensionContext:))]
        fn should_bypass_permissions(&self, _context: &WKWebExtensionContext) -> bool {
            false
        }
    }
);

impl BrowserTab {
    fn accepts(&self, context: &WKWebExtensionContext) -> bool {
        self.ivars().broker.accepts(None, context)
            && self
                .ivars()
                .guest
                .borrow()
                .as_ref()
                .is_none_or(|(owner, broker)| {
                    std::ptr::eq(&**owner, context)
                        && broker
                            .upgrade()
                            .is_some_and(|broker| broker.view(self.id(), context).is_some())
                })
    }
    fn bind_guest(
        &self,
        context: Retained<WKWebExtensionContext>,
        broker: &Rc<ExtensionPageBroker>,
        view: &Retained<WKWebView>,
    ) {
        *self.ivars().guest.borrow_mut() = Some((context, Rc::downgrade(broker)));
        *self.ivars().webview.borrow_mut() = Some(Weak::from_retained(view));
    }
    fn begin_unit_action(
        &self,
        context: &WKWebExtensionContext,
        completion: &block2::DynBlock<dyn Fn(*mut NSError)>,
        action: ExtensionBrowserRequestAction,
    ) {
        if !self.accepts(context) {
            self.ivars()
                .broker
                .reject_unit(completion, ExtensionBrowserRequestRejection::InvalidContext);
            return;
        }
        self.ivars().broker.begin_unit(action, completion);
    }

    fn new(
        mtm: MainThreadMarker,
        projected: &zephium_core::extensions::ExtensionBrowserTab,
        broker: Rc<BrowserRequestBroker>,
        #[cfg(feature = "native-web-extension-probes")] lifecycle_drops: Arc<AtomicUsize>,
    ) -> Result<Retained<Self>, BrowserSurfaceError> {
        let url = projected.url().map(native_url).transpose()?;
        let object = Self::alloc(mtm).set_ivars(BrowserTabIvars {
            id: Box::new(projected.id()),
            window: RefCell::new(None),
            webview: RefCell::new(None),
            title: RefCell::new(NSString::from_str(projected.title())),
            url: RefCell::new(url),
            attempted_url: RefCell::new(None),
            index: Cell::new(0),
            selected: Cell::new(false),
            resident: Cell::new(projected.resident()),
            loading: Cell::new(projected.loading()),
            pinned: Cell::new(projected.pinned()),
            broker,
            guest: RefCell::new(None),
            #[cfg(feature = "native-web-extension-probes")]
            lifecycle_drops,
        });
        // SAFETY: NSObject is the declared superclass and every ivar is fully
        // initialized before invoking its initializer.
        Ok(unsafe { msg_send![super(object), init] })
    }

    fn id(&self) -> ItemId {
        *self.ivars().id
    }

    fn is_resident(&self) -> bool {
        self.ivars().resident.get()
    }

    fn update(
        &self,
        window: &Retained<BrowserWindow>,
        index: usize,
        selected: bool,
        projected: &zephium_core::extensions::ExtensionBrowserTab,
        webview: Option<&Retained<WKWebView>>,
    ) -> Result<WKWebExtensionTabChangedProperties, BrowserSurfaceError> {
        let title_changed = !native_string_matches(&self.ivars().title.borrow(), projected.title());
        let url_changed =
            !native_url_matches(self.ivars().url.borrow().as_deref(), projected.url());
        let loading_changed = self.ivars().loading.get() != projected.loading();
        let pinned_changed = self.ivars().pinned.get() != projected.pinned();
        *self.ivars().window.borrow_mut() = Some(Weak::from_retained(window));
        if self.ivars().guest.borrow().is_none() {
            *self.ivars().webview.borrow_mut() = webview.map(Weak::from_retained);
        }
        if title_changed {
            *self.ivars().title.borrow_mut() = NSString::from_str(projected.title());
        }
        if url_changed {
            *self.ivars().url.borrow_mut() = projected.url().map(native_url).transpose()?;
        }
        if url_changed {
            self.ivars().attempted_url.borrow_mut().take();
        }
        self.ivars().index.set(index);
        self.ivars().selected.set(selected);
        self.ivars().resident.set(projected.resident());
        self.ivars().loading.set(projected.loading());
        self.ivars().pinned.set(projected.pinned());
        Ok(tab_property_change_flags(
            title_changed,
            url_changed,
            loading_changed,
            pinned_changed,
        ))
    }

    fn bind_webview(&self, webview: Option<&Retained<WKWebView>>) {
        if self.ivars().guest.borrow().is_none() {
            *self.ivars().webview.borrow_mut() = webview.map(Weak::from_retained);
        }
    }

    fn observe_url_attempt(
        &self,
        webview: &WKWebView,
        target: &str,
    ) -> Result<bool, BrowserSurfaceError> {
        let reason = if !self.ivars().resident.get() {
            Some("nonresident")
        } else if self.ivars().guest.borrow().is_some() {
            Some("extension-guest")
        } else if !self
            .ivars()
            .webview
            .borrow()
            .as_ref()
            .and_then(Weak::load)
            .is_some_and(|bound| std::ptr::eq(&*bound, webview))
        {
            Some("physical-view-unbound-or-mismatched")
        } else {
            None
        };
        if let Some(reason) = reason {
            if extension_tab_trace_enabled() {
                eprintln!("extension-tab-trace: native-tab outcome={reason}");
            }
            return Ok(false);
        }
        *self.ivars().attempted_url.borrow_mut() = Some(native_url(target)?);
        Ok(true)
    }

    fn clear_url_attempt(&self, webview: &WKWebView) {
        if self
            .ivars()
            .webview
            .borrow()
            .as_ref()
            .and_then(Weak::load)
            .is_some_and(|bound| std::ptr::eq(&*bound, webview))
        {
            self.ivars().attempted_url.borrow_mut().take();
        }
    }

    fn has_bound_resident_webview(&self) -> bool {
        self.is_resident()
            && self
                .ivars()
                .webview
                .borrow()
                .as_ref()
                .and_then(Weak::load)
                .is_some()
    }

    fn index(&self) -> usize {
        self.ivars().index.get()
    }

    fn window(&self) -> Option<Retained<BrowserWindow>> {
        self.ivars().window.borrow().as_ref().and_then(Weak::load)
    }

    fn begin_activate(
        &self,
        context: &WKWebExtensionContext,
        completion: &block2::DynBlock<dyn Fn(*mut NSError)>,
    ) {
        if !self.accepts(context) {
            self.ivars()
                .broker
                .reject_unit(completion, ExtensionBrowserRequestRejection::InvalidContext);
            return;
        }
        self.ivars().broker.begin_unit(
            ExtensionBrowserRequestAction::ActivateTab { tab: self.id() },
            completion,
        );
    }

    fn reject_unsupported(
        &self,
        context: &WKWebExtensionContext,
        completion: &block2::DynBlock<dyn Fn(*mut NSError)>,
    ) {
        let reason = if self.accepts(context) {
            ExtensionBrowserRequestRejection::Unsupported
        } else {
            ExtensionBrowserRequestRejection::InvalidContext
        };
        self.ivars().broker.reject_unit(completion, reason);
    }
}

fn native_url(value: &str) -> Result<Retained<NSURL>, BrowserSurfaceError> {
    NSURL::URLWithString(&NSString::from_str(value)).ok_or(BrowserSurfaceError::IntegrityFailed)
}

fn native_string_matches(native: &NSString, expected: &str) -> bool {
    objc2::rc::autoreleasepool(|pool| {
        // SAFETY: the pool outlives this borrowed UTF-8 view, which is used
        // only for the equality check inside the autorelease scope.
        unsafe { native.to_str(pool) == expected }
    })
}

fn native_url_matches(native: Option<&NSURL>, expected: Option<&str>) -> bool {
    match (native, expected) {
        (None, None) => true,
        (Some(native), Some(expected)) => native
            .absoluteString()
            .is_some_and(|value| native_string_matches(&value, expected)),
        _ => false,
    }
}

fn extension_tab_trace_enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| std::env::var("ZEPHIUM_EXTENSION_TAB_TRACE").as_deref() == Ok("1"))
}

fn trace_error_domain(error: &NSError) -> &'static str {
    match error.domain().to_string().as_str() {
        "NSURLErrorDomain" => "nsurl",
        "WKErrorDomain" => "webkit",
        "NSPOSIXErrorDomain" => "posix",
        "WKWebExtensionContextErrorDomain" => "web_extension_context",
        _ => "other",
    }
}

fn trace_error_resource(value: &str) -> &'static str {
    if value.contains("__zephium_background_v1.js") || value.contains("__zephium_background_v2.js")
    {
        "background_wrapper"
    } else if value.contains("webkit-api-v1.js") {
        "api_prelude"
    } else if value.contains("webkit-runtime-messaging-v2.js") {
        "runtime_messaging"
    } else if value.contains("webkit-offscreen-background-v1.js") {
        "offscreen_bridge"
    } else if value.contains("webkit-notifications-v1.js") {
        "notifications_bridge"
    } else if value.contains("webkit-managed-storage-v1.js") {
        "managed_storage_bridge"
    } else if value.contains("webkit-web-navigation-v1.js") {
        "web_navigation_bridge"
    } else if value.contains("background.js") {
        "publisher_background"
    } else if value.contains("offscreen-document/") {
        "offscreen_document"
    } else {
        "unknown"
    }
}

fn trace_error_exception(value: &str) -> &'static str {
    for (needle, class) in [
        ("TypeError", "type"),
        ("ReferenceError", "reference"),
        ("SyntaxError", "syntax"),
        ("SecurityError", "security"),
        ("NotSupportedError", "unsupported"),
        ("NetworkError", "network"),
        ("AbortError", "abort"),
    ] {
        if value.contains(needle) {
            return class;
        }
    }
    "unknown"
}

#[cfg(feature = "native-extension-qa-inspector")]
pub(super) fn redacted_background_error_text(value: &str) -> String {
    let mut output = String::with_capacity(320);
    for word in value.split_whitespace() {
        let lower = word.to_ascii_lowercase();
        let sensitive_assignment = [
            "token=",
            "code=",
            "state=",
            "password=",
            "secret=",
            "session=",
            "cookie=",
            "authorization=",
            "email=",
        ]
        .iter()
        .any(|key| lower.contains(key));
        let redact = lower.contains("://")
            || lower.starts_with("data:")
            || lower.starts_with("blob:")
            || lower.contains('@')
            || sensitive_assignment
            || word.len() > 96
            || (word.len() > 32 && word.bytes().filter(u8::is_ascii_digit).count() >= 4);
        let safe = if redact { "[redacted]" } else { word };
        if output.len() + safe.len() + 1 > 320 {
            output.push_str(" ...");
            break;
        }
        if !output.is_empty() {
            output.push(' ');
        }
        for byte in safe.bytes() {
            output.push(if byte.is_ascii_graphic() {
                char::from(byte)
            } else {
                '_'
            });
        }
    }
    if output.is_empty() {
        output.push_str("<empty>");
    }
    output
}

#[cfg(feature = "native-extension-qa-inspector")]
fn trace_background_error_text(error: &NSError) {
    static REPORTED: OnceLock<()> = OnceLock::new();
    if REPORTED.set(()).is_err() {
        return;
    }
    let outer = redacted_background_error_text(&error.localizedDescription().to_string());
    let user_info = error.userInfo();
    let underlying_object = user_info.objectForKey(unsafe { NSUnderlyingErrorKey });
    let underlying = underlying_object
        .as_deref()
        .and_then(|value| value.downcast_ref::<NSError>());
    let nested = underlying.map_or_else(
        || "<absent>".to_owned(),
        |cause| redacted_background_error_text(&cause.localizedDescription().to_string()),
    );
    let failure = user_info
        .objectForKey(unsafe { NSLocalizedFailureReasonErrorKey })
        .as_deref()
        .and_then(|value| value.downcast_ref::<NSString>())
        .map_or_else(
            || "<absent>".to_owned(),
            |reason| redacted_background_error_text(&reason.to_string()),
        );
    eprintln!(
        "extension-tab-trace: background-load-error description={outer} underlying={nested} reason={failure}"
    );
}

fn trace_context_error_cause(error: &NSError) -> (&'static str, isize, &'static str, &'static str) {
    let user_info = error.userInfo();
    let underlying_object = user_info.objectForKey(unsafe { NSUnderlyingErrorKey });
    let underlying = underlying_object
        .as_deref()
        .and_then(|value| value.downcast_ref::<NSError>());
    let cause = underlying.unwrap_or(error);
    let description = cause.localizedDescription().to_string();
    let failing_path = cause
        .userInfo()
        .objectForKey(unsafe { NSURLErrorFailingURLErrorKey })
        .as_deref()
        .and_then(|value| value.downcast_ref::<NSURL>())
        .and_then(NSURL::path)
        .map(|path| path.to_string());
    let resource = failing_path
        .as_deref()
        .map(trace_error_resource)
        .filter(|class| *class != "unknown")
        .unwrap_or_else(|| trace_error_resource(&description));
    (
        if underlying.is_some() {
            trace_error_domain(cause)
        } else {
            "absent"
        },
        if underlying.is_some() {
            cause.code()
        } else {
            0
        },
        resource,
        trace_error_exception(&description),
    )
}

fn callback_extension_id(target: &str) -> Option<String> {
    let parsed = url::Url::parse(target).ok()?;
    if parsed.scheme() != "https" {
        return None;
    }
    let host = parsed.host_str()?;
    let id = host.strip_suffix(".chromiumapp.org")?;
    (id.len() == 32 && id.bytes().all(|byte| matches!(byte, b'a'..=b'p'))).then(|| id.to_owned())
}

pub(super) fn request_url(url: &NSURL) -> Result<std::sync::Arc<str>, ()> {
    let value = url.absoluteString().ok_or(())?;
    if value.lengthOfBytesUsingEncoding(NSUTF8StringEncoding)
        > MAX_EXTENSION_BROWSER_REQUEST_URL_BYTES
    {
        return Err(());
    }
    objc2::rc::autoreleasepool(|pool| {
        // SAFETY: the borrowed UTF-8 view is consumed inside this pool.
        let value = unsafe { value.to_str(pool) };
        if !zephium_core::navigation::is_allowed_str(value) {
            return Err(());
        }
        Ok(std::sync::Arc::from(value))
    })
}

#[cfg(any(
    feature = "native-extension-product-probes",
    feature = "native-extension-lab-diagnostics"
))]
fn product_probe_create_diagnostic(
    reason: &'static str,
    configuration: &WKWebExtensionTabConfiguration,
) {
    // The debug-only probe prints configuration shape, never extension data,
    // URLs, titles, paths, or native error descriptions.
    unsafe {
        eprintln!(
            "extension-product-probe-tabs-create: reason={reason}; index={}; window={}; parent={}; active={}; selected={}; pinned={}; muted={}; reader={}",
            configuration.index(),
            configuration.window().is_some(),
            configuration.parentTab().is_some(),
            configuration.shouldBeActive(),
            configuration.shouldAddToSelection(),
            configuration.shouldBePinned(),
            configuration.shouldBeMuted(),
            configuration.shouldReaderModeBeActive(),
        );
    }
}

#[cfg(not(any(
    feature = "native-extension-product-probes",
    feature = "native-extension-lab-diagnostics"
)))]
fn product_probe_create_diagnostic(
    _reason: &'static str,
    _configuration: &WKWebExtensionTabConfiguration,
) {
}

#[cfg(feature = "native-web-extension-probes")]
impl Drop for BrowserTab {
    fn drop(&mut self) {
        self.ivars().lifecycle_drops.fetch_add(1, Ordering::Relaxed);
    }
}

struct BrowserWindowIvars {
    id: WindowId,
    private: bool,
    tabs: RefCell<Vec<Retained<BrowserTab>>>,
    active: RefCell<Option<Retained<BrowserTab>>>,
    #[cfg(feature = "native-web-extension-probes")]
    lifecycle_drops: Arc<AtomicUsize>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "ZephiumExtensionBrowserWindow"]
    #[ivars = BrowserWindowIvars]
    struct BrowserWindow;

    unsafe impl NSObjectProtocol for BrowserWindow {}

    unsafe impl WKWebExtensionWindow for BrowserWindow {
        #[unsafe(method_id(tabsForWebExtensionContext:))]
        fn tabs_for_context(
            &self,
            _context: &WKWebExtensionContext,
        ) -> Retained<NSArray<ProtocolObject<dyn WKWebExtensionTab>>> {
            let tabs = self
                .ivars()
                .tabs
                .borrow()
                .iter()
                .cloned()
                .map(ProtocolObject::from_retained)
                .collect::<Vec<_>>();
            NSArray::from_retained_slice(&tabs)
        }

        #[unsafe(method_id(activeTabForWebExtensionContext:))]
        fn active_tab_for_context(
            &self,
            _context: &WKWebExtensionContext,
        ) -> Option<Retained<ProtocolObject<dyn WKWebExtensionTab>>> {
            self.ivars()
                .active
                .borrow()
                .as_ref()
                .cloned()
                .map(ProtocolObject::from_retained)
        }

        #[unsafe(method(isPrivateForWebExtensionContext:))]
        fn is_private(&self, _context: &WKWebExtensionContext) -> bool {
            self.ivars().private
        }
    }
);

impl BrowserWindow {
    fn new(
        mtm: MainThreadMarker,
        id: WindowId,
        private: bool,
        #[cfg(feature = "native-web-extension-probes")] lifecycle_drops: Arc<AtomicUsize>,
    ) -> Retained<Self> {
        let object = Self::alloc(mtm).set_ivars(BrowserWindowIvars {
            id,
            private,
            tabs: RefCell::new(Vec::new()),
            active: RefCell::new(None),
            #[cfg(feature = "native-web-extension-probes")]
            lifecycle_drops,
        });
        // SAFETY: NSObject is the declared superclass and every ivar is fully
        // initialized before invoking its initializer.
        unsafe { msg_send![super(object), init] }
    }

    fn id(&self) -> WindowId {
        self.ivars().id
    }

    fn private_class(&self) -> bool {
        self.ivars().private
    }

    fn active_id(&self) -> Option<ItemId> {
        self.ivars().active.borrow().as_ref().map(|tab| tab.id())
    }

    fn replace_tabs(&self, tabs: Vec<Retained<BrowserTab>>, active: Option<ItemId>) {
        let active = active.and_then(|active| tabs.iter().find(|tab| tab.id() == active).cloned());
        *self.ivars().tabs.borrow_mut() = tabs;
        *self.ivars().active.borrow_mut() = active;
    }

    fn tab_count(&self) -> usize {
        self.ivars().tabs.borrow().len()
    }
}

#[cfg(feature = "native-web-extension-probes")]
impl Drop for BrowserWindow {
    fn drop(&mut self) {
        self.ivars().lifecycle_drops.fetch_add(1, Ordering::Relaxed);
    }
}

struct BrowserControllerDelegateIvars {
    windows: RefCell<Vec<Retained<BrowserWindow>>>,
    focused: RefCell<Option<Retained<BrowserWindow>>>,
    broker: Rc<BrowserRequestBroker>,
    action_popup: Rc<ActionPopupBroker>,
    extension_pages: Rc<ExtensionPageBroker>,
    runtime_grants: Rc<RuntimeGrantRequestBroker>,
    compatibility_broker: Rc<CompatibilityBroker>,
    identity_broker: Rc<IdentityBroker>,
    offscreen_profile: Box<ProfileId>,
    offscreen_sink: Option<crate::EngineEventIngressSink>,
    offscreen_broker: RefCell<Option<Rc<OffscreenBroker>>>,
    publisher_native_messaging: Rc<PublisherNativeMessagingBroker>,
    #[cfg(feature = "native-web-extension-probes")]
    lifecycle_drops: Arc<AtomicUsize>,
}

struct BrowserControllerDelegateServices {
    broker: Rc<BrowserRequestBroker>,
    action_popup: Rc<ActionPopupBroker>,
    extension_pages: Rc<ExtensionPageBroker>,
    runtime_grants: Rc<RuntimeGrantRequestBroker>,
    compatibility_broker: Rc<CompatibilityBroker>,
    identity_broker: Rc<IdentityBroker>,
    offscreen_profile: ProfileId,
    offscreen_sink: Option<crate::EngineEventIngressSink>,
    publisher_native_messaging: Rc<PublisherNativeMessagingBroker>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "ZephiumExtensionBrowserControllerDelegate"]
    #[ivars = BrowserControllerDelegateIvars]
    struct BrowserControllerDelegate;

    unsafe impl NSObjectProtocol for BrowserControllerDelegate {}

    unsafe impl WKWebExtensionControllerDelegate for BrowserControllerDelegate {
        #[unsafe(method_id(webExtensionController:openWindowsForExtensionContext:))]
        fn open_windows(
            &self,
            _controller: &WKWebExtensionController,
            _context: &WKWebExtensionContext,
        ) -> Retained<NSArray<ProtocolObject<dyn WKWebExtensionWindow>>> {
            let windows = self
                .ivars()
                .windows
                .borrow()
                .iter()
                .cloned()
                .map(ProtocolObject::from_retained)
                .collect::<Vec<_>>();
            NSArray::from_retained_slice(&windows)
        }

        #[unsafe(method_id(webExtensionController:focusedWindowForExtensionContext:))]
        fn focused_window(
            &self,
            _controller: &WKWebExtensionController,
            _context: &WKWebExtensionContext,
        ) -> Option<Retained<ProtocolObject<dyn WKWebExtensionWindow>>> {
            self.ivars()
                .focused
                .borrow()
                .as_ref()
                .cloned()
                .map(ProtocolObject::from_retained)
        }

        #[unsafe(method(webExtensionController:openNewTabUsingConfiguration:forExtensionContext:completionHandler:))]
        fn open_new_tab(
            &self,
            controller: &WKWebExtensionController,
            configuration: &WKWebExtensionTabConfiguration,
            context: &WKWebExtensionContext,
            completion: &block2::DynBlock<
                dyn Fn(*mut ProtocolObject<dyn WKWebExtensionTab>, *mut NSError),
            >,
        ) {
            let broker = &self.ivars().broker;
            if !broker.accepts(Some(controller), context) {
                product_probe_create_diagnostic("invalid-context", configuration);
                broker.reject_tab(completion, ExtensionBrowserRequestRejection::InvalidContext);
                return;
            }
            if let Some(url) = unsafe { configuration.url() } {
                if self.ivars().extension_pages.accepts_url(context, &url) {
                    let supported = unsafe {
                        extension_page_configuration_supported(
                            configuration.parentTab().is_some(),
                            configuration.shouldBePinned(),
                            configuration.shouldBeMuted(),
                            configuration.shouldReaderModeBeActive(),
                            configuration.shouldBeActive(),
                            configuration.shouldAddToSelection(),
                        )
                    };
                    if !supported {
                        product_probe_create_diagnostic(
                            "extension-page-unsupported-configuration",
                            configuration,
                        );
                        broker
                            .reject_tab(completion, ExtensionBrowserRequestRejection::Unsupported);
                        return;
                    }
                    let target_count = match unsafe { configuration.window() } {
                        None => self
                            .ivars()
                            .focused
                            .borrow()
                            .as_ref()
                            .map(|window| window.tab_count()),
                        Some(requested) => {
                            self.window_for(&requested).map(|window| window.tab_count())
                        }
                    };
                    let index = unsafe { configuration.index() };
                    if target_count.is_none()
                        || (index != NSNotFound as usize && target_count != Some(index))
                    {
                        product_probe_create_diagnostic(
                            "extension-page-invalid-target",
                            configuration,
                        );
                        broker
                            .reject_tab(completion, ExtensionBrowserRequestRejection::InvalidScope);
                        return;
                    }
                    product_probe_create_diagnostic("extension-page", configuration);
                    let Some(context) = (unsafe {
                        Retained::retain(context as *const _ as *mut WKWebExtensionContext)
                    }) else {
                        broker.reject_tab(
                            completion,
                            ExtensionBrowserRequestRejection::NativeAdmissionFailed,
                        );
                        return;
                    };
                    broker.begin_extension_page(context, url, completion);
                    return;
                }
            }
            // SAFETY: all configuration properties are immutable snapshots
            // supplied to this main-thread delegate callback.
            let unsupported = unsafe {
                configuration.parentTab().is_some()
                    || configuration.shouldBePinned()
                    || configuration.shouldBeMuted()
                    || configuration.shouldReaderModeBeActive()
                    || (configuration.shouldAddToSelection() && !configuration.shouldBeActive())
            };
            if unsupported {
                product_probe_create_diagnostic("unsupported-configuration", configuration);
                broker.reject_tab(completion, ExtensionBrowserRequestRejection::Unsupported);
                return;
            }
            // SAFETY: the immutable optional protocol object remains retained
            // while we resolve it against the delegate's exact window graph.
            let (window, target_window) = match unsafe { configuration.window() } {
                None => (None, self.ivars().focused.borrow().clone()),
                Some(requested) => {
                    let Some(window) = self.window_for(&requested) else {
                        product_probe_create_diagnostic("invalid-window", configuration);
                        broker
                            .reject_tab(completion, ExtensionBrowserRequestRejection::InvalidScope);
                        return;
                    };
                    (Some(window.id()), Some(window))
                }
            };
            // WebKit resolves an omitted Chrome `index` to the current append
            // position. Zephium's Shell append operation is therefore exact
            // for either NSNotFound or that target window's current length;
            // arbitrary insertion remains explicitly unsupported.
            let index = unsafe { configuration.index() };
            if index != NSNotFound as usize
                && target_window.as_ref().map(|window| window.tab_count()) != Some(index)
            {
                product_probe_create_diagnostic("unsupported-index", configuration);
                broker.reject_tab(completion, ExtensionBrowserRequestRejection::Unsupported);
                return;
            }
            // SAFETY: URL is an immutable retained configuration property.
            let url = match unsafe { configuration.url() } {
                None => None,
                Some(url) => match request_url(&url) {
                    Ok(url) => Some(url),
                    Err(()) => {
                        product_probe_create_diagnostic("invalid-url", configuration);
                        broker.reject_tab(
                            completion,
                            ExtensionBrowserRequestRejection::InvalidRequest,
                        );
                        return;
                    }
                },
            };
            // SAFETY: boolean configuration properties are immutable here.
            let active = unsafe { configuration.shouldBeActive() };
            product_probe_create_diagnostic("ordinary", configuration);
            broker.begin_tab(
                ExtensionBrowserRequestAction::CreateTab {
                    window,
                    url,
                    active,
                },
                completion,
            );
        }

        #[unsafe(method(webExtensionController:openOptionsPageForExtensionContext:completionHandler:))]
        fn open_options_page(
            &self,
            controller: &WKWebExtensionController,
            context: &WKWebExtensionContext,
            completion: &block2::DynBlock<dyn Fn(*mut NSError)>,
        ) {
            crate::diagnostic!("extensions: native options-page request received");
            if !self.ivars().broker.accepts(Some(controller), context) {
                self.ivars()
                    .broker
                    .reject_unit(completion, ExtensionBrowserRequestRejection::InvalidContext);
                return;
            }
            self.ivars()
                .action_popup
                .open_options_page(controller, context, completion);
        }

        #[unsafe(method(webExtensionController:didUpdateAction:forExtensionContext:))]
        unsafe fn did_update_action(
            &self,
            controller: &WKWebExtensionController,
            action: &WKWebExtensionAction,
            context: &WKWebExtensionContext,
        ) {
            let broker = &self.ivars().broker;
            if !broker.accepts(Some(controller), context) {
                return;
            }
            let Some(action_context) = (unsafe { action.webExtensionContext() }) else {
                return;
            };
            if !std::ptr::eq(&*action_context, context) {
                return;
            }
            if let Some(tab) = unsafe { action.associatedTab() } {
                let known = self.ivars().windows.borrow().iter().any(|window| {
                    window
                        .ivars()
                        .tabs
                        .borrow()
                        .iter()
                        .any(|candidate| ProtocolObject::from_ref(&**candidate) == &*tab)
                });
                if !known {
                    return;
                }
            }
            broker.notify_actions_invalidated();
        }

        #[unsafe(method(webExtensionController:presentPopupForAction:forExtensionContext:completionHandler:))]
        unsafe fn present_popup(
            &self,
            controller: &WKWebExtensionController,
            action: &WKWebExtensionAction,
            context: &WKWebExtensionContext,
            completion: &block2::DynBlock<dyn Fn(*mut NSError)>,
        ) {
            self.ivars()
                .action_popup
                .present(controller, action, context, completion);
        }

        #[unsafe(method(webExtensionController:promptForPermissions:inTab:forExtensionContext:completionHandler:))]
        fn prompt_for_permissions(
            &self,
            controller: &WKWebExtensionController,
            permissions: &NSSet<WKWebExtensionPermission>,
            _tab: Option<&ProtocolObject<dyn WKWebExtensionTab>>,
            context: &WKWebExtensionContext,
            completion: &block2::DynBlock<
                dyn Fn(NonNull<NSSet<WKWebExtensionPermission>>, *mut NSDate),
            >,
        ) {
            self.ivars().runtime_grants.begin_permissions(
                controller,
                context,
                permissions,
                completion,
            );
        }

        #[unsafe(method(webExtensionController:promptForPermissionMatchPatterns:inTab:forExtensionContext:completionHandler:))]
        fn prompt_for_patterns(
            &self,
            controller: &WKWebExtensionController,
            patterns: &NSSet<WKWebExtensionMatchPattern>,
            _tab: Option<&ProtocolObject<dyn WKWebExtensionTab>>,
            context: &WKWebExtensionContext,
            completion: &block2::DynBlock<
                dyn Fn(NonNull<NSSet<WKWebExtensionMatchPattern>>, *mut NSDate),
            >,
        ) {
            self.ivars()
                .runtime_grants
                .begin_patterns(controller, context, patterns, completion);
        }

        #[unsafe(method(webExtensionController:promptForPermissionToAccessURLs:inTab:forExtensionContext:completionHandler:))]
        fn prompt_for_urls(
            &self,
            _controller: &WKWebExtensionController,
            _urls: &NSSet<NSURL>,
            _tab: Option<&ProtocolObject<dyn WKWebExtensionTab>>,
            _context: &WKWebExtensionContext,
            completion: &block2::DynBlock<dyn Fn(NonNull<NSSet<NSURL>>, *mut NSDate)>,
        ) {
            // URL-set prompts are tab/document-scoped authority, not durable
            // manifest optional-host grants. Keep that separate capability
            // unavailable until its activeTab/document broker exists.
            let empty = NSSet::<NSURL>::new();
            completion.call((NonNull::from(&*empty), std::ptr::null_mut()));
        }

        #[unsafe(method(webExtensionController:sendMessage:toApplicationWithIdentifier:forExtensionContext:replyHandler:))]
        unsafe fn compatibility_message(
            &self,
            controller: &WKWebExtensionController,
            message: &AnyObject,
            application_identifier: Option<&NSString>,
            context: &WKWebExtensionContext,
            reply: &block2::DynBlock<dyn Fn(*mut AnyObject, *mut NSError)>,
        ) {
            if application_identifier.is_some_and(|identifier| {
                identifier.isEqualToString(&NSString::from_str(
                    EXTENSION_COMPATIBILITY_BROKER_APPLICATION_ID,
                ))
            }) {
                crate::diagnostic!("extensions: compatibility broker native request received");
                self.ivars()
                    .compatibility_broker
                    .begin(controller, context, message, reply);
                return;
            }
            if application_identifier
                .is_some_and(super::identity_broker::matches_application_identifier)
            {
                self.ivars()
                    .identity_broker
                    .begin(controller, context, message, reply);
                return;
            }
            self.ivars().publisher_native_messaging.begin_one_shot(
                controller,
                context,
                application_identifier,
                message,
                reply,
            );
        }

        #[unsafe(method(webExtensionController:connectUsingMessagePort:forExtensionContext:completionHandler:))]
        unsafe fn connect_native_message_port(
            &self,
            controller: &WKWebExtensionController,
            port: &WKWebExtensionMessagePort,
            context: &WKWebExtensionContext,
            completion: &block2::DynBlock<dyn Fn(*mut NSError)>,
        ) {
            if unsafe { port.applicationIdentifier() }
                .as_ref()
                .is_some_and(|id| OffscreenBroker::matches_application_identifier(id))
            {
                let broker = {
                    let mut slot = self.ivars().offscreen_broker.borrow_mut();
                    slot.get_or_insert_with(|| {
                        let broker = OffscreenBroker::new(
                            *self.ivars().offscreen_profile,
                            self.ivars().offscreen_sink.clone(),
                        );
                        if let Some(controller) = unsafe {
                            Retained::retain(std::ptr::NonNull::from(controller).as_ptr())
                        } {
                            broker.bind_controller(&controller);
                        }
                        broker
                    })
                    .clone()
                };
                broker.begin_port(controller, context, port, completion);
                return;
            }
            self.ivars()
                .publisher_native_messaging
                .begin_port(controller, context, port, completion);
        }
    }
);

impl BrowserControllerDelegate {
    fn new(
        mtm: MainThreadMarker,
        services: BrowserControllerDelegateServices,
        #[cfg(feature = "native-web-extension-probes")] lifecycle_drops: Arc<AtomicUsize>,
    ) -> Retained<Self> {
        let BrowserControllerDelegateServices {
            broker,
            action_popup,
            extension_pages,
            runtime_grants,
            compatibility_broker,
            identity_broker,
            offscreen_profile,
            offscreen_sink,
            publisher_native_messaging,
        } = services;
        let object = Self::alloc(mtm).set_ivars(BrowserControllerDelegateIvars {
            windows: RefCell::new(Vec::new()),
            focused: RefCell::new(None),
            broker,
            action_popup,
            extension_pages,
            runtime_grants,
            compatibility_broker,
            identity_broker,
            offscreen_profile: Box::new(offscreen_profile),
            offscreen_sink,
            offscreen_broker: RefCell::new(None),
            publisher_native_messaging,
            #[cfg(feature = "native-web-extension-probes")]
            lifecycle_drops,
        });
        // SAFETY: NSObject is the declared superclass and every ivar is fully
        // initialized before invoking its initializer.
        unsafe { msg_send![super(object), init] }
    }

    fn replace(
        &self,
        windows: Vec<Retained<BrowserWindow>>,
        focused: Option<Retained<BrowserWindow>>,
    ) {
        *self.ivars().windows.borrow_mut() = windows;
        *self.ivars().focused.borrow_mut() = focused;
    }

    fn window_for(
        &self,
        requested: &ProtocolObject<dyn WKWebExtensionWindow>,
    ) -> Option<Retained<BrowserWindow>> {
        self.ivars().windows.borrow().iter().find_map(|window| {
            (ProtocolObject::from_ref(&**window) == requested).then(|| window.clone())
        })
    }
}

#[cfg(feature = "native-web-extension-probes")]
impl Drop for BrowserControllerDelegate {
    fn drop(&mut self) {
        self.ivars().lifecycle_drops.fetch_add(1, Ordering::Relaxed);
    }
}

pub(super) struct MacosExtensionBrowserSurfaceHost {
    profile: ProfileId,
    generation: Option<ExtensionBrowserSurfaceGeneration>,
    delegate: Retained<BrowserControllerDelegate>,
    broker: Rc<BrowserRequestBroker>,
    action_popup: Rc<ActionPopupBroker>,
    extension_pages: Rc<ExtensionPageBroker>,
    runtime_grants: Rc<RuntimeGrantRequestBroker>,
    compatibility_broker: Rc<CompatibilityBroker>,
    identity_broker: Rc<IdentityBroker>,
    publisher_native_messaging: Rc<PublisherNativeMessagingBroker>,
    windows: HashMap<WindowId, Retained<BrowserWindow>>,
    tabs: HashMap<ItemId, Retained<BrowserTab>>,
    #[cfg(feature = "native-web-extension-probes")]
    lifecycle_drops: Arc<AtomicUsize>,
}

impl MacosExtensionBrowserSurfaceHost {
    pub(super) fn new(
        profile: ProfileId,
        sink: Option<crate::EngineEventIngressSink>,
        request_pool: Rc<BrowserRequestPool>,
        runtime_grant_pool: Rc<RuntimeGrantRequestPool>,
        compatibility_broker_pool: Rc<CompatibilityBrokerPool>,
        native_host_process_pool: std::sync::Arc<NativeHostProcessPool>,
    ) -> Result<Self, BrowserSurfaceError> {
        let mtm = MainThreadMarker::new().ok_or(BrowserSurfaceError::MainThreadRequired)?;
        let broker = BrowserRequestBroker::new(profile, sink.clone(), request_pool);
        let action_popup = ActionPopupBroker::new(profile, sink.clone(), Rc::clone(&broker));
        let extension_pages = ExtensionPageBroker::new(Rc::clone(&broker));
        let runtime_grants =
            RuntimeGrantRequestBroker::new(profile, sink.clone(), runtime_grant_pool);
        let compatibility_broker =
            CompatibilityBroker::new(profile, sink.clone(), compatibility_broker_pool);
        let identity_broker = IdentityBroker::new(profile);
        let publisher_native_messaging =
            PublisherNativeMessagingBroker::new(profile, native_host_process_pool);
        #[cfg(feature = "native-web-extension-probes")]
        let lifecycle_drops = Arc::new(AtomicUsize::new(0));
        Ok(Self {
            profile,
            generation: None,
            delegate: BrowserControllerDelegate::new(
                mtm,
                BrowserControllerDelegateServices {
                    broker: broker.clone(),
                    action_popup: action_popup.clone(),
                    extension_pages: extension_pages.clone(),
                    runtime_grants: runtime_grants.clone(),
                    compatibility_broker: compatibility_broker.clone(),
                    identity_broker: identity_broker.clone(),
                    offscreen_profile: profile,
                    offscreen_sink: sink.clone(),
                    publisher_native_messaging: publisher_native_messaging.clone(),
                },
                #[cfg(feature = "native-web-extension-probes")]
                lifecycle_drops.clone(),
            ),
            broker,
            action_popup,
            extension_pages,
            runtime_grants,
            compatibility_broker,
            identity_broker,
            publisher_native_messaging,
            windows: HashMap::new(),
            tabs: HashMap::new(),
            #[cfg(feature = "native-web-extension-probes")]
            lifecycle_drops,
        })
    }

    pub(super) fn attach(&self, controller: &Retained<WKWebExtensionController>) {
        self.broker.bind_controller(controller);
        self.extension_pages.bind_controller(controller);
        self.runtime_grants.bind_controller(controller);
        self.compatibility_broker.bind_controller(controller);
        self.identity_broker.bind_controller(controller);
        self.publisher_native_messaging.bind_controller(controller);
        let delegate = ProtocolObject::from_ref(&*self.delegate);
        // SAFETY: the host retains the main-thread delegate for at least as
        // long as the owning controller entry remains live.
        unsafe { controller.setDelegate(Some(delegate)) };
    }

    pub(super) fn is_attached(&self, controller: &WKWebExtensionController) -> bool {
        let expected = ProtocolObject::from_ref(&*self.delegate);
        // SAFETY: delegate readback is a public main-thread WebKit property.
        unsafe { controller.delegate() }.is_some_and(|actual| &*actual == expected)
    }

    pub(super) const fn generation(&self) -> Option<ExtensionBrowserSurfaceGeneration> {
        self.generation
    }

    /// Invalidates Shell action metadata after an extension runtime generation
    /// changes without mutating the browser surface itself.
    pub(super) fn notify_actions_invalidated_after_activation(&self) {
        self.broker.notify_actions_invalidated_after_activation();
    }

    pub(super) fn notify_actions_invalidated(&self) {
        self.broker.notify_actions_invalidated();
    }

    pub(super) fn open_options_page_from_browser(
        &self,
        context: Retained<WKWebExtensionContext>,
        completion: &block2::DynBlock<
            dyn Fn(*mut ProtocolObject<dyn WKWebExtensionTab>, *mut objc2_foundation::NSError),
        >,
    ) -> Result<(), ExtensionActionRejection> {
        self.action_popup
            .open_options_page_from_browser(context, completion)
    }

    pub(super) fn apply(
        &mut self,
        controller: &WKWebExtensionController,
        surface: &ExtensionBrowserSurface,
        mut webview_for: impl FnMut(ItemId) -> Option<Retained<WKWebView>>,
    ) -> Result<(), BrowserSurfaceError> {
        if surface.profile() != self.profile {
            return Err(BrowserSurfaceError::ProfileMismatch);
        }
        if self
            .generation
            .is_some_and(|generation| surface.generation() <= generation)
        {
            return Err(BrowserSurfaceError::StaleGeneration);
        }
        let mtm = MainThreadMarker::new().ok_or(BrowserSurfaceError::MainThreadRequired)?;
        let old_windows = std::mem::take(&mut self.windows);
        let old_tabs = std::mem::take(&mut self.tabs);
        let old_focused = self
            .delegate
            .ivars()
            .focused
            .borrow()
            .as_ref()
            .map(|window| window.id());
        let old_active = old_windows
            .iter()
            .map(|(id, window)| (*id, window.active_id()))
            .collect::<HashMap<_, _>>();
        let old_tab_positions = old_tabs
            .iter()
            .map(|(id, tab)| (*id, (tab.window().map(|window| window.id()), tab.index())))
            .collect::<HashMap<_, _>>();

        let mut changed_tabs = HashMap::new();
        let mut windows = HashMap::with_capacity(surface.windows().len());
        let mut tabs = HashMap::with_capacity(surface.tabs().count());
        let mut ordered_windows = Vec::with_capacity(surface.windows().len());
        for projected in surface.windows() {
            let window = match old_windows.get(&projected.id()) {
                Some(window) if window.private_class() == projected.is_private() => window.clone(),
                Some(_) => return Err(BrowserSurfaceError::PrivacyClassChanged),
                None => BrowserWindow::new(
                    mtm,
                    projected.id(),
                    projected.is_private(),
                    #[cfg(feature = "native-web-extension-probes")]
                    self.lifecycle_drops.clone(),
                ),
            };
            let mut ordered_tabs = Vec::with_capacity(projected.tabs().len());
            for (index, projected_tab) in projected.tabs().iter().enumerate() {
                let tab = match old_tabs.get(&projected_tab.id()) {
                    Some(tab) => tab.clone(),
                    None => BrowserTab::new(
                        mtm,
                        projected_tab,
                        self.broker.clone(),
                        #[cfg(feature = "native-web-extension-probes")]
                        self.lifecycle_drops.clone(),
                    )?,
                };
                let selected = projected.active() == Some(projected_tab.id());
                let webview = projected_tab
                    .resident()
                    .then(|| webview_for(projected_tab.id()))
                    .flatten();
                let changed =
                    tab.update(&window, index, selected, projected_tab, webview.as_ref())?;
                if old_tabs.contains_key(&projected_tab.id()) && !changed.is_empty() {
                    changed_tabs.insert(projected_tab.id(), changed);
                }
                if let Some((context, view)) = self.extension_pages.binding(projected_tab.id()) {
                    tab.bind_guest(context, &self.extension_pages, &view);
                }
                if tabs.insert(projected_tab.id(), tab.clone()).is_some() {
                    return Err(BrowserSurfaceError::IntegrityFailed);
                }
                ordered_tabs.push(tab);
            }
            window.replace_tabs(ordered_tabs, projected.active());
            if windows.insert(projected.id(), window.clone()).is_some() {
                return Err(BrowserSurfaceError::IntegrityFailed);
            }
            ordered_windows.push(window);
        }
        let focused = surface
            .focused()
            .and_then(|focused| windows.get(&focused).cloned());

        // Publish one internally consistent graph before any WebKit callback
        // can synchronously query the delegate.
        self.delegate.replace(ordered_windows, focused.clone());
        self.windows = windows;
        self.tabs = tabs;
        self.generation = Some(surface.generation());

        let current = focused
            .as_ref()
            .and_then(|window| window.active_id())
            .and_then(|id| {
                self.tabs
                    .get(&id)
                    .map(|tab| (id, tab.has_bound_resident_webview()))
            });
        self.action_popup.reconcile_tab(current);

        for (id, tab) in &old_tabs {
            if !self.tabs.contains_key(id) {
                let window_is_closing = tab
                    .window()
                    .is_some_and(|window| !self.windows.contains_key(&window.id()));
                let protocol = ProtocolObject::from_ref(&**tab);
                // SAFETY: old objects remain retained through this balanced
                // close notification and the replacement graph is complete.
                unsafe { controller.didCloseTab_windowIsClosing(protocol, window_is_closing) };
            }
        }
        for (id, window) in &old_windows {
            if !self.windows.contains_key(id) {
                let protocol = ProtocolObject::from_ref(&**window);
                // SAFETY: the removed window remains retained through the
                // balanced close notification.
                unsafe { controller.didCloseWindow(protocol) };
            }
        }
        for projected in surface.windows() {
            let window = self
                .windows
                .get(&projected.id())
                .ok_or(BrowserSurfaceError::IntegrityFailed)?;
            if !old_windows.contains_key(&projected.id()) {
                let protocol = ProtocolObject::from_ref(&**window);
                // SAFETY: the delegate graph already exposes this exact
                // retained object.
                unsafe { controller.didOpenWindow(protocol) };
            }
            for tab in projected.tabs() {
                let native_tab = self
                    .tabs
                    .get(&tab.id())
                    .ok_or(BrowserSurfaceError::IntegrityFailed)?;
                let protocol = ProtocolObject::from_ref(&**native_tab);
                if !old_tabs.contains_key(&tab.id()) {
                    // SAFETY: the delegate graph already exposes this exact
                    // retained object.
                    unsafe { controller.didOpenTab(protocol) };
                } else if old_tab_positions
                    .get(&tab.id())
                    .is_some_and(|(old_window, old_index)| {
                        *old_window != Some(projected.id()) || *old_index != native_tab.index()
                    })
                {
                    let (old_window_id, old_index) = old_tab_positions[&tab.id()];
                    let old_window = old_window_id
                        .and_then(|id| old_windows.get(&id))
                        .map(|window| ProtocolObject::from_ref(&**window));
                    // SAFETY: both old and new graph objects remain retained
                    // for the duration of the move notification.
                    unsafe {
                        controller.didMoveTab_fromIndex_inWindow(protocol, old_index, old_window)
                    };
                }
                if let Some(changed) = changed_tabs.get(&tab.id()).copied() {
                    // The replacement graph is visible before WebKit can ask
                    // for the updated URL or loading state. WebKit does not
                    // infer tabs.onUpdated from a changed delegate getter.
                    unsafe { controller.didChangeTabProperties_forTab(changed, protocol) };
                }
            }
            let previous = old_active.get(&projected.id()).copied().flatten();
            if projected.active() != previous {
                if let Some(active) = projected.active() {
                    let active = self
                        .tabs
                        .get(&active)
                        .ok_or(BrowserSurfaceError::IntegrityFailed)?;
                    let previous = previous
                        .and_then(|id| old_tabs.get(&id))
                        .map(|tab| ProtocolObject::from_ref(&**tab));
                    // SAFETY: the active object belongs to the published
                    // graph; a previous object remains retained by `old_tabs`.
                    unsafe {
                        controller.didActivateTab_previousActiveTab(
                            ProtocolObject::from_ref(&**active),
                            previous,
                        )
                    };
                }
            }
        }
        if surface.focused() != old_focused {
            let focused = focused
                .as_ref()
                .map(|window| ProtocolObject::from_ref(&**window));
            // SAFETY: the optional focused object belongs to the published
            // retained delegate graph.
            unsafe { controller.didFocusWindow(focused) };
        }
        Ok(())
    }

    /// Reports when the effective action target changes between the default
    /// action and this tab. Shell must refresh its revision after that change.
    pub(super) fn bind_webview(&self, id: ItemId, webview: Option<&Retained<WKWebView>>) -> bool {
        let Some(tab) = self.tabs.get(&id) else {
            return false;
        };
        let was_bound = tab.has_bound_resident_webview();
        tab.bind_webview(webview);
        let changed = was_bound != tab.has_bound_resident_webview();
        if changed {
            let current = self
                .delegate
                .ivars()
                .focused
                .borrow()
                .as_ref()
                .and_then(|window| window.active_id())
                .and_then(|active| {
                    self.tabs
                        .get(&active)
                        .map(|tab| (active, tab.has_bound_resident_webview()))
                });
            self.action_popup.reconcile_tab(current);
        }
        changed
    }

    pub(super) fn observe_browser_tab_url_attempt(
        &self,
        controller: &WKWebExtensionController,
        id: ItemId,
        webview: &WKWebView,
        target: &str,
    ) -> Result<bool, BrowserSurfaceError> {
        let Some(tab) = self.tabs.get(&id) else {
            if extension_tab_trace_enabled() {
                eprintln!("extension-tab-trace: native-tab outcome=logical-tab-absent");
            }
            return Ok(false);
        };
        if !tab.observe_url_attempt(webview, target)? {
            return Ok(false);
        }
        if extension_tab_trace_enabled() {
            let callback_id = callback_extension_id(target);
            let native_target = native_url(target)?;
            let protocol = ProtocolObject::from_ref(&**tab);
            let mut matching_context = false;
            let mut context_loaded = false;
            let mut url_granted = false;
            let mut context_errors = 0_usize;
            let mut context_error_class = "none";
            let mut context_error_code = 0_isize;
            let mut underlying_class = "absent";
            let mut underlying_code = 0_isize;
            let mut error_resource = "unknown";
            let mut exception_class = "unknown";
            for context in unsafe { controller.extensionContexts() }.iter() {
                let matches = callback_id
                    .as_ref()
                    .is_none_or(|id| unsafe { context.uniqueIdentifier() }.to_string() == *id);
                if matches {
                    matching_context = true;
                    context_loaded |= unsafe { context.isLoaded() };
                    url_granted |=
                        unsafe { context.hasAccessToURL_inTab(&native_target, Some(protocol)) };
                    let errors = unsafe { context.errors() };
                    context_errors = context_errors.saturating_add(errors.count());
                    if errors.count() > 0 {
                        let error = errors.objectAtIndex(errors.count() - 1);
                        context_error_class = if error
                            .domain()
                            .isEqualToString(unsafe { WKWebExtensionContextErrorDomain })
                        {
                            "web_extension_context"
                        } else {
                            "other"
                        };
                        context_error_code = error.code();
                        #[cfg(feature = "native-extension-qa-inspector")]
                        if context_error_class == "web_extension_context" && context_error_code == 6
                        {
                            trace_background_error_text(&error);
                        }
                        (
                            underlying_class,
                            underlying_code,
                            error_resource,
                            exception_class,
                        ) = trace_context_error_cause(&error);
                    }
                }
            }
            let origin = match callback_id.as_ref() {
                Some(_) if matching_context => "expected_callback",
                Some(_) => "other_chromiumapp",
                None if target.starts_with("https://") => "other_https",
                None if target.starts_with("http://") => "other_http",
                None => "other_scheme",
            };
            let callback_shape = callback_id.as_ref().and_then(|id| {
                let parsed = url::Url::parse(target).ok()?;
                let expected_prefix = format!("https://{id}.chromiumapp.org");
                let prefix_match = target.starts_with(&expected_prefix);
                let path = if parsed.path() == "/" {
                    "root"
                } else {
                    "non_root"
                };
                let mut has_code = false;
                let mut has_state = false;
                let mut has_error = false;
                for (key, _) in parsed.query_pairs() {
                    match key.as_ref() {
                        "code" => has_code = true,
                        "state" => has_state = true,
                        "error" => has_error = true,
                        _ => {}
                    }
                }
                Some((prefix_match, path, has_code, has_state, has_error))
            });
            let (prefix_match, callback_path, has_code, has_state, has_error) =
                callback_shape.unwrap_or((false, "none", false, false, false));
            eprintln!(
                "extension-tab-trace: native-publish tab={id} origin={origin} context_match={matching_context} context_loaded={context_loaded} url_granted={url_granted} context_errors={context_errors} context_error_class={context_error_class} context_error_code={context_error_code} underlying_class={underlying_class} underlying_code={underlying_code} error_resource={error_resource} exception_class={exception_class} callback_prefix_match={prefix_match} callback_path={callback_path} has_code={has_code} has_state={has_state} has_error={has_error}"
            );
        }
        // The tab delegate exposes this URL only as an in-flight native
        // attempt. WebKit applies each context's own tabs/host permissions;
        // Shell's committed browsing URL and document grants never move here.
        unsafe {
            controller.didChangeTabProperties_forTab(
                WKWebExtensionTabChangedProperties::URL,
                ProtocolObject::from_ref(&**tab),
            )
        };
        Ok(true)
    }

    pub(super) fn clear_browser_tab_url_attempt(&self, id: ItemId, webview: &WKWebView) {
        if let Some(tab) = self.tabs.get(&id) {
            tab.clear_url_attempt(webview);
        }
    }

    pub(super) fn is_ready_for_document_background(&self) -> bool {
        document_background_surface_ready(
            self.generation.is_some(),
            self.tabs
                .values()
                .map(|tab| (tab.is_resident(), tab.has_bound_resident_webview())),
        )
    }

    /// Resolves one logical tab inside the exact already-published generation.
    /// The native-action bit is true only while its already-bound renderer is
    /// live; checking the weak view never creates or restores a renderer.
    pub(super) fn action_tab(
        &self,
        generation: ExtensionBrowserSurfaceGeneration,
        id: ItemId,
    ) -> Result<Option<(NativeExtensionTab, bool)>, BrowserSurfaceError> {
        if self.generation != Some(generation) {
            return Err(BrowserSurfaceError::StaleGeneration);
        }
        Ok(self.tabs.get(&id).cloned().map(|tab| {
            let resident = tab.has_bound_resident_webview();
            (ProtocolObject::from_retained(tab), resident)
        }))
    }

    pub(super) fn settle_request(
        &self,
        request: zephium_core::extensions::ExtensionBrowserRequestId,
        settlement: zephium_core::extensions::ExtensionBrowserRequestSettlement,
        extension_page_lease: Option<crate::host::NativeResourceLease>,
        extension_page_stage: Option<(
            Retained<crate::platform::imp::ContentStage>,
            std::sync::Arc<std::sync::atomic::AtomicBool>,
        )>,
    ) -> BrowserRequestSettlementOutcome {
        let created_id = match settlement {
            zephium_core::extensions::ExtensionBrowserRequestSettlement::Applied(
                zephium_core::extensions::ExtensionBrowserRequestResult::CreatedTab(id),
            ) => Some(id),
            _ => None,
        };
        let created = match settlement {
            zephium_core::extensions::ExtensionBrowserRequestSettlement::Applied(
                zephium_core::extensions::ExtensionBrowserRequestResult::CreatedTab(id),
            ) => self
                .tabs
                .get(&id)
                .map(|tab| ProtocolObject::from_ref(&**tab)),
            _ => None,
        };
        let page_tab = match settlement {
            zephium_core::extensions::ExtensionBrowserRequestSettlement::Applied(
                zephium_core::extensions::ExtensionBrowserRequestResult::ExtensionPageAuthorized {
                    tab,
                    window,
                },
            ) => self
                .tabs
                .get(&tab)
                .filter(|native| native.window().is_some_and(|actual| actual.id() == window))
                .cloned(),
            _ => None,
        };
        let outcome = self.broker.settle(
            request,
            settlement,
            created,
            extension_page_lease,
            |context, url, lease| {
                let tab = page_tab
                    .as_ref()
                    .ok_or(ExtensionBrowserRequestRejection::InvalidScope)?;
                let (stage, permit) = extension_page_stage
                    .ok_or(ExtensionBrowserRequestRejection::NativeAdmissionFailed)?;
                let view = self.extension_pages.present(
                    context.clone(),
                    url,
                    lease,
                    tab.id(),
                    stage,
                    permit,
                )?;
                tab.bind_guest(context, &self.extension_pages, &view);
                Ok(ProtocolObject::from_retained(tab.clone()))
            },
        );
        if extension_tab_trace_enabled() {
            if let Some(id) = created_id {
                eprintln!(
                    "extension-tab-trace: native-created-tab-reply tab={id} request={request:?} outcome={outcome:?}"
                );
            }
        }
        if let Some(tab) = page_tab {
            if tab.ivars().guest.borrow().is_none() {
                self.broker.extension_page_closed(tab.id());
            }
        }
        outcome
    }

    pub(super) fn has_extension_page(&self, id: ItemId) -> bool {
        self.extension_pages.contains(id)
    }
    pub(super) fn close_extension_page(&self, id: ItemId) -> bool {
        self.extension_pages.close_item(id)
    }
    pub(super) fn navigate_extension_page(&self, id: ItemId, action: u8) -> bool {
        self.extension_pages.navigation(id, action)
    }

    pub(super) fn timeout_request(
        &self,
        request: zephium_core::extensions::ExtensionBrowserRequestId,
    ) -> bool {
        self.broker.timeout(request)
    }

    pub(super) fn runtime_grant_context_identity(
        &self,
        request: zephium_core::ports::extensions::ExtensionRuntimeGrantRequestId,
    ) -> Option<*const WKWebExtensionContext> {
        self.runtime_grants.pending_context_identity(request)
    }

    pub(super) fn finalize_runtime_grant_request(
        &self,
        request: zephium_core::ports::extensions::ExtensionRuntimeGrantRequestId,
        subject: Option<(
            zephium_core::extensions::ExtensionRuntimeInstance,
            zephium_core::extensions::ExtensionNativeOwnershipKey,
            String,
        )>,
    ) -> bool {
        self.runtime_grants.finalize(request, subject)
    }

    pub(super) fn settle_runtime_grant_request(
        &self,
        runtime: zephium_core::extensions::ExtensionRuntimeInstance,
        request: zephium_core::ports::extensions::ExtensionRuntimeGrantRequestId,
        settlement: zephium_core::ports::extensions::ExtensionRuntimeGrantPromptSettlement,
    ) -> RuntimeGrantSettlementOutcome {
        self.runtime_grants.settle(runtime, request, settlement)
    }

    pub(super) fn timeout_runtime_grant_request(
        &self,
        request: zephium_core::ports::extensions::ExtensionRuntimeGrantRequestId,
    ) -> bool {
        self.runtime_grants.timeout(request)
    }

    pub(super) fn cancel_runtime_grant_context(&self, context: *const WKWebExtensionContext) {
        self.runtime_grants.cancel_context(context);
    }

    pub(super) fn compatibility_broker_context_identity(
        &self,
        request: ExtensionCompatibilityBrokerRequestId,
    ) -> Option<*const WKWebExtensionContext> {
        self.compatibility_broker.pending_context_identity(request)
    }

    pub(super) fn compatibility_broker_operation(
        &self,
        request: ExtensionCompatibilityBrokerRequestId,
    ) -> Option<zephium_core::extensions::ExtensionCompatibilityBrokerOperation> {
        self.compatibility_broker.pending_operation(request)
    }

    pub(super) fn finalize_compatibility_broker_request(
        &self,
        request: ExtensionCompatibilityBrokerRequestId,
        witness: Option<ExtensionCompatibilityBrokerWitness>,
    ) -> bool {
        self.compatibility_broker.finalize(request, witness)
    }

    pub(super) fn settle_compatibility_broker_request(
        &self,
        runtime: zephium_core::extensions::ExtensionRuntimeInstance,
        request: ExtensionCompatibilityBrokerRequestId,
        settlement: ExtensionCompatibilityBrokerSettlement,
    ) -> CompatibilityBrokerSettlementOutcome {
        self.compatibility_broker
            .settle(runtime, request, settlement, |context, reply| {
                self.action_popup
                    .open_options_page_authorized(context, reply)
            })
    }

    pub(super) fn timeout_compatibility_broker_request(
        &self,
        request: ExtensionCompatibilityBrokerRequestId,
    ) -> bool {
        self.compatibility_broker.timeout(request)
    }

    pub(super) fn cancel_compatibility_broker_context(
        &self,
        context: *const WKWebExtensionContext,
    ) {
        self.compatibility_broker.cancel_context(context);
    }

    pub(super) fn identity_request_context(
        &self,
        request: IdentityRequestId,
    ) -> Option<*const WKWebExtensionContext> {
        self.identity_broker.pending_context(request)
    }

    pub(super) fn finalize_identity_request(
        &self,
        request: IdentityRequestId,
        witness: Option<ExtensionCompatibilityBrokerWitness>,
        lease: Option<crate::host::NativeResourceLease>,
    ) -> bool {
        self.identity_broker.finalize(request, witness, lease)
    }

    pub(super) fn timeout_identity_request(&self, request: IdentityRequestId) -> bool {
        self.identity_broker.timeout(request)
    }

    pub(super) fn cancel_identity_context(&self, context: *const WKWebExtensionContext) {
        self.identity_broker.cancel_context(context);
    }

    pub(super) fn offscreen_subject(
        &self,
        request: OffscreenSessionId,
    ) -> Option<(*const WKWebExtensionContext, bool)> {
        self.delegate
            .ivars()
            .offscreen_broker
            .borrow()
            .as_ref()?
            .subject(request)
    }

    pub(super) fn pending_offscreen_authorization_ids(&self) -> Vec<OffscreenSessionId> {
        self.delegate
            .ivars()
            .offscreen_broker
            .borrow()
            .as_ref()
            .map_or_else(Vec::new, |broker| broker.pending_authorization_ids())
    }

    pub(super) fn authorize_offscreen(
        &self,
        request: OffscreenSessionId,
        witness: Option<ExtensionCompatibilityBrokerWitness>,
        lease: Option<crate::host::NativeResourceLease>,
    ) -> bool {
        self.delegate
            .ivars()
            .offscreen_broker
            .borrow()
            .as_ref()
            .is_some_and(|broker| broker.authorize(request, witness, lease))
    }

    pub(super) fn settle_offscreen_resource(
        &self,
        runtime: zephium_core::extensions::ExtensionRuntimeInstance,
        request: u64,
        outcome: zephium_core::ports::extensions::IsolatedExtensionResourceOutcome,
    ) -> bool {
        self.delegate
            .ivars()
            .offscreen_broker
            .borrow()
            .as_ref()
            .is_some_and(|broker| broker.settle_resource(runtime, request, outcome))
    }

    pub(super) fn offscreen_resource_context(
        &self,
        runtime: zephium_core::extensions::ExtensionRuntimeInstance,
    ) -> Option<*const WKWebExtensionContext> {
        self.delegate
            .ivars()
            .offscreen_broker
            .borrow()
            .as_ref()?
            .resource_context(runtime)
    }

    pub(super) fn cancel_offscreen_context(&self, context: *const WKWebExtensionContext) {
        if let Some(broker) = self.delegate.ivars().offscreen_broker.borrow().as_ref() {
            broker.cancel_context(context);
        }
    }

    pub(super) fn native_messaging_subject(
        &self,
        request: PublisherNativeMessagingRequestId,
    ) -> Option<(*const WKWebExtensionContext, Box<str>)> {
        self.publisher_native_messaging.pending_subject(request)
    }

    pub(super) fn reserve_native_messaging_authorization_retry(
        &self,
        request: PublisherNativeMessagingRequestId,
    ) -> bool {
        self.publisher_native_messaging
            .reserve_authorization_retry(request)
    }

    pub(super) fn authorize_native_messaging(
        &self,
        request: PublisherNativeMessagingRequestId,
        authorization: Option<PublisherNativeMessagingAuthorization>,
    ) -> bool {
        self.publisher_native_messaging
            .authorize(request, authorization)
    }

    pub(super) fn handle_native_messaging_worker_event(
        &self,
        request: PublisherNativeMessagingRequestId,
        event: super::native_messaging::NativeHostWorkerEvent,
    ) -> bool {
        self.publisher_native_messaging
            .handle_worker_event(request, event)
    }

    pub(super) fn timeout_native_messaging(
        &self,
        request: PublisherNativeMessagingRequestId,
    ) -> bool {
        self.publisher_native_messaging.timeout(request)
    }

    pub(super) fn cancel_native_messaging_context(&self, context: *const WKWebExtensionContext) {
        self.publisher_native_messaging.cancel_context(context);
    }

    pub(super) fn begin_action_popup(
        &self,
        request: ExtensionActionRequest,
        controller: Retained<WKWebExtensionController>,
        owner: super::native_runtime::MacosNativeActionPopupOwner,
        tab: Option<NativeExtensionTab>,
        parent: Retained<NSView>,
        lease: crate::host::NativeResourceLease,
    ) -> Result<(), ExtensionActionRejection> {
        self.action_popup
            .begin(request, controller, owner, tab, parent, lease)
    }

    pub(super) fn prepare_action_popup(
        &self,
        request: ExtensionActionRequest,
        context: &WKWebExtensionContext,
        tab: Option<&NativeExtensionTab>,
    ) -> Result<ActionPopupPreparation, ExtensionActionRejection> {
        self.action_popup.prepare_toggle(request, context, tab)
    }

    pub(super) fn timeout_action_popup(&self, request: ExtensionActionRequestId) -> bool {
        self.action_popup.timeout(request)
    }

    pub(super) fn cancel_action_popup_context(
        &self,
        context: *const WKWebExtensionContext,
        reason: ExtensionActionRejection,
    ) {
        self.action_popup.cancel_context(context, reason);
        self.extension_pages.cancel_context(context);
    }

    #[cfg(feature = "native-web-extension-probes")]
    pub(super) fn diagnostics(&self) -> BrowserSurfaceDiagnostics {
        BrowserSurfaceDiagnostics {
            discarded_tab_webview_refusals: self.broker.discarded_tab_webview_refusals(),
        }
    }

    #[cfg(feature = "native-web-extension-probes")]
    pub(super) fn probe_identity(
        &self,
        window: WindowId,
        tab: ItemId,
    ) -> Option<ProbeBrowserSurfaceIdentity> {
        Some((
            ProtocolObject::from_retained(self.windows.get(&window)?.clone()),
            ProtocolObject::from_retained(self.tabs.get(&tab)?.clone()),
        ))
    }

    #[cfg(feature = "native-web-extension-probes")]
    pub(super) fn probe_lifecycle_drops(&self) -> Arc<AtomicUsize> {
        self.lifecycle_drops.clone()
    }

    /// Removes the complete logical graph after every extension context for
    /// this controller has retired. This is a native-lifecycle operation, not
    /// a Shell generation: a tombstoned profile cannot publish another
    /// surface, and shutdown has already sealed ingress.
    pub(super) fn clear(&mut self, controller: &WKWebExtensionController) {
        self.action_popup.seal_and_close();
        self.extension_pages.seal_and_close();
        self.broker.seal_and_reject();
        self.runtime_grants.seal_and_reject();
        self.compatibility_broker.seal_and_reject();
        self.identity_broker.seal_and_reject();
        if let Some(broker) = self.delegate.ivars().offscreen_broker.borrow_mut().take() {
            broker.seal();
        }
        self.publisher_native_messaging.seal_and_reject();
        let old_windows = std::mem::take(&mut self.windows);
        let old_tabs = std::mem::take(&mut self.tabs);
        let had_focus = self.delegate.ivars().focused.borrow().is_some();

        // Make every synchronous delegate query observe the terminal empty
        // graph before notifying WebKit about its retired objects.
        self.delegate.replace(Vec::new(), None);
        for tab in old_tabs.values() {
            let protocol = ProtocolObject::from_ref(&**tab);
            // SAFETY: every old tab and its window remain retained by the
            // local maps through this balanced terminal notification.
            unsafe { controller.didCloseTab_windowIsClosing(protocol, true) };
        }
        for window in old_windows.values() {
            let protocol = ProtocolObject::from_ref(&**window);
            // SAFETY: the old window remains retained through the callback.
            unsafe { controller.didCloseWindow(protocol) };
        }
        if had_focus {
            // SAFETY: `nil` is the documented representation of no focused
            // extension window.
            unsafe { controller.didFocusWindow(None) };
        }
    }
}

fn document_background_surface_ready(
    has_generation: bool,
    tabs: impl IntoIterator<Item = (bool, bool)>,
) -> bool {
    has_generation && tabs.into_iter().any(|(resident, bound)| resident && bound)
}

fn tab_property_change_flags(
    title: bool,
    url: bool,
    loading: bool,
    pinned: bool,
) -> WKWebExtensionTabChangedProperties {
    let mut changed = WKWebExtensionTabChangedProperties::None;
    if title {
        changed |= WKWebExtensionTabChangedProperties::Title;
    }
    if url {
        changed |= WKWebExtensionTabChangedProperties::URL;
    }
    if loading {
        changed |= WKWebExtensionTabChangedProperties::Loading;
    }
    if pinned {
        changed |= WKWebExtensionTabChangedProperties::Pinned;
    }
    changed
}

/// Accepts the only extension-page shape Zephium can represent exactly.
///
/// Zephium selects one active tab. Both active selection flags therefore
/// select the created tab; background, parent, pinned, muted and reader-mode
/// requests require semantics this surface does not yet represent.
const fn extension_page_configuration_supported(
    has_parent: bool,
    pinned: bool,
    muted: bool,
    reader_mode: bool,
    active: bool,
    _add_to_selection: bool,
) -> bool {
    !has_parent && !pinned && !muted && !reader_mode && active
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_delegate_graph_remains_main_thread_only() {
        fn assert_main_thread_only<T: MainThreadOnly>() {}
        assert_main_thread_only::<BrowserTab>();
        assert_main_thread_only::<BrowserWindow>();
        assert_main_thread_only::<BrowserControllerDelegate>();
    }

    #[test]
    fn navigation_changes_publish_the_url_and_loading_properties() {
        let change = tab_property_change_flags(false, true, true, false);
        assert!(change.contains(WKWebExtensionTabChangedProperties::URL));
        assert!(change.contains(WKWebExtensionTabChangedProperties::Loading));
        assert!(!change.contains(WKWebExtensionTabChangedProperties::Title));
        assert_eq!(
            tab_property_change_flags(false, false, false, false),
            WKWebExtensionTabChangedProperties::None,
        );
    }

    #[test]
    fn discarded_tab_refusal_never_resolves_a_native_view() {
        let broker =
            BrowserRequestBroker::new(ProfileId::from(1), None, Rc::new(BrowserRequestPool::new()));
        let resolver_called = Cell::new(false);
        let resolved = resolve_resident_webview(false, &broker, || {
            resolver_called.set(true);
            None
        });
        assert!(resolved.is_none());
        assert!(!resolver_called.get());
        assert_eq!(broker.discarded_tab_webview_refusals(), 1);
    }

    #[test]
    fn resident_tab_uses_the_existing_view_resolver_without_false_refusal() {
        let broker =
            BrowserRequestBroker::new(ProfileId::from(1), None, Rc::new(BrowserRequestPool::new()));
        let resolver_called = Cell::new(false);
        let resolved = resolve_resident_webview(true, &broker, || {
            resolver_called.set(true);
            None
        });
        assert!(resolved.is_none());
        assert!(resolver_called.get());
        assert_eq!(broker.discarded_tab_webview_refusals(), 0);
    }

    #[test]
    fn document_background_requires_a_published_resident_native_surface() {
        assert!(!document_background_surface_ready(false, [(true, true)]));
        assert!(!document_background_surface_ready(
            true,
            [(false, true), (true, false)]
        ));
        assert!(document_background_surface_ready(
            true,
            [(false, true), (true, true)]
        ));
    }

    #[test]
    fn extension_page_accepts_chrome_default_without_inventing_selection() {
        assert!(extension_page_configuration_supported(
            false, false, false, false, true, false,
        ));
        assert!(extension_page_configuration_supported(
            false, false, false, false, true, true,
        ));

        for rejected in [
            (true, false, false, false, true, false),
            (false, true, false, false, true, false),
            (false, false, true, false, true, false),
            (false, false, false, true, true, false),
            (false, false, false, false, false, false),
        ] {
            assert!(!extension_page_configuration_supported(
                rejected.0, rejected.1, rejected.2, rejected.3, rejected.4, rejected.5,
            ));
        }
    }

    #[cfg(feature = "native-extension-qa-inspector")]
    #[test]
    fn qa_background_error_keeps_exception_but_redacts_auth_values() {
        let text = redacted_background_error_text(
            "TypeError: Cannot read properties of undefined (reading 'storage') at https://auth.example/callback?code=private token=secret email=person@example.test",
        );
        assert!(text.contains("TypeError"));
        assert!(text.contains("'storage'"));
        assert!(!text.contains("auth.example"));
        assert!(!text.contains("private"));
        assert!(!text.contains("secret"));
        assert!(!text.contains("person@example.test"));
    }
}
