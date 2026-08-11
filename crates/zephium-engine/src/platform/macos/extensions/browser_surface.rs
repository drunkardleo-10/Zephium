//! Main-thread WKWebExtension window/tab delegate graph.
//!
//! The graph mirrors Shell-owned logical identities but never creates a
//! `WKWebView`. A tab callback returns a native view only when the Shell marks
//! the tab resident and the engine already owns that exact physical view.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;
#[cfg(feature = "native-web-extension-probes")]
use std::sync::atomic::{AtomicUsize, Ordering};
#[cfg(feature = "native-web-extension-probes")]
use std::sync::Arc;

use objc2::rc::{Retained, Weak};
use objc2::runtime::{NSObject, ProtocolObject};
use objc2::{define_class, msg_send, DefinedClass, MainThreadOnly};
use objc2_foundation::{
    MainThreadMarker, NSArray, NSError, NSNotFound, NSObjectProtocol, NSString,
    NSUTF8StringEncoding, NSURL,
};
use objc2_web_kit::{
    WKWebExtensionAction, WKWebExtensionContext, WKWebExtensionController,
    WKWebExtensionControllerDelegate, WKWebExtensionTab, WKWebExtensionTabConfiguration,
    WKWebExtensionWindow, WKWebView,
};
use zephium_core::extensions::{
    ExtensionBrowserRequestAction, ExtensionBrowserRequestRejection, ExtensionBrowserSurface,
    ExtensionBrowserSurfaceGeneration, MAX_EXTENSION_BROWSER_REQUEST_URL_BYTES,
};
use zephium_core::ids::{ItemId, ProfileId, WindowId};

pub(super) type NativeExtensionTab = Retained<ProtocolObject<dyn WKWebExtensionTab>>;

use super::browser_request_broker::{
    BrowserRequestBroker, BrowserRequestPool, BrowserRequestSettlementOutcome,
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
#[cfg(any(test, feature = "native-web-extension-probes"))]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct BrowserSurfaceDiagnostics {
    discarded_tab_webview_refusals: u64,
}

#[cfg(any(test, feature = "native-web-extension-probes"))]
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
    index: Cell<usize>,
    selected: Cell<bool>,
    resident: Cell<bool>,
    loading: Cell<bool>,
    pinned: Cell<bool>,
    broker: Rc<BrowserRequestBroker>,
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
            _context: &WKWebExtensionContext,
        ) -> Option<Retained<WKWebView>> {
            resolve_resident_webview(self.ivars().resident.get(), &self.ivars().broker, || {
                self.ivars().webview.borrow().as_ref().and_then(Weak::load)
            })
        }

        #[unsafe(method_id(titleForWebExtensionContext:))]
        fn title_for_context(
            &self,
            _context: &WKWebExtensionContext,
        ) -> Option<Retained<NSString>> {
            Some(self.ivars().title.borrow().clone())
        }

        #[unsafe(method_id(urlForWebExtensionContext:))]
        fn url_for_context(&self, _context: &WKWebExtensionContext) -> Option<Retained<NSURL>> {
            self.ivars().url.borrow().clone()
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
            if !self.ivars().broker.accepts(None, context) {
                self.ivars()
                    .broker
                    .reject_unit(completion, ExtensionBrowserRequestRejection::InvalidContext);
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
            if !self.ivars().broker.accepts(None, context) {
                self.ivars()
                    .broker
                    .reject_unit(completion, ExtensionBrowserRequestRejection::InvalidContext);
                return;
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
            _from_origin: bool,
            context: &WKWebExtensionContext,
            completion: &block2::DynBlock<dyn Fn(*mut NSError)>,
        ) {
            self.reject_unsupported(context, completion);
        }

        #[unsafe(method(goBackForWebExtensionContext:completionHandler:))]
        fn go_back(
            &self,
            context: &WKWebExtensionContext,
            completion: &block2::DynBlock<dyn Fn(*mut NSError)>,
        ) {
            self.reject_unsupported(context, completion);
        }

        #[unsafe(method(goForwardForWebExtensionContext:completionHandler:))]
        fn go_forward(
            &self,
            context: &WKWebExtensionContext,
            completion: &block2::DynBlock<dyn Fn(*mut NSError)>,
        ) {
            self.reject_unsupported(context, completion);
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
            index: Cell::new(0),
            selected: Cell::new(false),
            resident: Cell::new(projected.resident()),
            loading: Cell::new(projected.loading()),
            pinned: Cell::new(projected.pinned()),
            broker,
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
    ) -> Result<(), BrowserSurfaceError> {
        *self.ivars().window.borrow_mut() = Some(Weak::from_retained(window));
        *self.ivars().webview.borrow_mut() = webview.map(Weak::from_retained);
        if !native_string_matches(&self.ivars().title.borrow(), projected.title()) {
            *self.ivars().title.borrow_mut() = NSString::from_str(projected.title());
        }
        if !native_url_matches(self.ivars().url.borrow().as_deref(), projected.url()) {
            *self.ivars().url.borrow_mut() = projected.url().map(native_url).transpose()?;
        }
        self.ivars().index.set(index);
        self.ivars().selected.set(selected);
        self.ivars().resident.set(projected.resident());
        self.ivars().loading.set(projected.loading());
        self.ivars().pinned.set(projected.pinned());
        Ok(())
    }

    fn bind_webview(&self, webview: Option<&Retained<WKWebView>>) {
        *self.ivars().webview.borrow_mut() = webview.map(Weak::from_retained);
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
        if !self.ivars().broker.accepts(None, context) {
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
        let reason = if self.ivars().broker.accepts(None, context) {
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

fn request_url(url: &NSURL) -> Result<std::sync::Arc<str>, ()> {
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

#[cfg(feature = "native-extension-product-probes")]
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

#[cfg(not(feature = "native-extension-product-probes"))]
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
    #[cfg(feature = "native-web-extension-probes")]
    lifecycle_drops: Arc<AtomicUsize>,
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
            broker.begin_tab(
                ExtensionBrowserRequestAction::CreateTab {
                    window,
                    url,
                    active,
                },
                completion,
            );
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
    }
);

impl BrowserControllerDelegate {
    fn new(
        mtm: MainThreadMarker,
        broker: Rc<BrowserRequestBroker>,
        #[cfg(feature = "native-web-extension-probes")] lifecycle_drops: Arc<AtomicUsize>,
    ) -> Retained<Self> {
        let object = Self::alloc(mtm).set_ivars(BrowserControllerDelegateIvars {
            windows: RefCell::new(Vec::new()),
            focused: RefCell::new(None),
            broker,
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
    ) -> Result<Self, BrowserSurfaceError> {
        let mtm = MainThreadMarker::new().ok_or(BrowserSurfaceError::MainThreadRequired)?;
        let broker = BrowserRequestBroker::new(profile, sink, request_pool);
        #[cfg(feature = "native-web-extension-probes")]
        let lifecycle_drops = Arc::new(AtomicUsize::new(0));
        Ok(Self {
            profile,
            generation: None,
            delegate: BrowserControllerDelegate::new(
                mtm,
                broker.clone(),
                #[cfg(feature = "native-web-extension-probes")]
                lifecycle_drops.clone(),
            ),
            broker,
            windows: HashMap::new(),
            tabs: HashMap::new(),
            #[cfg(feature = "native-web-extension-probes")]
            lifecycle_drops,
        })
    }

    pub(super) fn attach(&self, controller: &Retained<WKWebExtensionController>) {
        self.broker.bind_controller(controller);
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
                tab.update(&window, index, selected, projected_tab, webview.as_ref())?;
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

    pub(super) fn bind_webview(&self, id: ItemId, webview: Option<&Retained<WKWebView>>) {
        if let Some(tab) = self.tabs.get(&id) {
            tab.bind_webview(webview);
        }
    }

    /// Resolves one logical tab inside the exact already-published generation.
    /// This clones only the retained protocol wrapper and never consults the
    /// tab's weak webview, so action enumeration cannot resurrect a discarded
    /// renderer.
    pub(super) fn action_tab(
        &self,
        generation: ExtensionBrowserSurfaceGeneration,
        id: ItemId,
    ) -> Result<Option<(NativeExtensionTab, bool)>, BrowserSurfaceError> {
        if self.generation != Some(generation) {
            return Err(BrowserSurfaceError::StaleGeneration);
        }
        Ok(self.tabs.get(&id).cloned().map(|tab| {
            let resident = tab.is_resident();
            (ProtocolObject::from_retained(tab), resident)
        }))
    }

    pub(super) fn settle_request(
        &self,
        request: zephium_core::extensions::ExtensionBrowserRequestId,
        settlement: zephium_core::extensions::ExtensionBrowserRequestSettlement,
    ) -> BrowserRequestSettlementOutcome {
        let created = match settlement {
            zephium_core::extensions::ExtensionBrowserRequestSettlement::Applied(
                zephium_core::extensions::ExtensionBrowserRequestResult::CreatedTab(id),
            ) => self
                .tabs
                .get(&id)
                .map(|tab| ProtocolObject::from_ref(&**tab)),
            _ => None,
        };
        self.broker.settle(request, settlement, created)
    }

    pub(super) fn timeout_request(
        &self,
        request: zephium_core::extensions::ExtensionBrowserRequestId,
    ) -> bool {
        self.broker.timeout(request)
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
        self.broker.seal_and_reject();
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
}
