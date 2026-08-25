//! Bounded same-principal extension documents presented outside ordinary tabs.
//!
//! `webkit-extension:` is never admitted into Zephium's ordinary navigation
//! model. A loaded `WKWebExtensionContext` may instead request one internal
//! document in this profile-scoped trust zone. The native request retains the
//! exact context and URL until the Shell authorizes the foreground profile;
//! presentation then requires a separately accounted foreground-extension
//! resource lease. Foreign principals, ambient web navigation and page-world
//! authority never cross this boundary.

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak as RcWeak};

use block2::{DynBlock, RcBlock};
use objc2::rc::{Retained, Weak};
use objc2::runtime::{NSObject, ProtocolObject};
use objc2::{define_class, msg_send, DefinedClass, MainThreadOnly};
use objc2_app_kit::{
    NSAutoresizingMaskOptions, NSBackingStoreType, NSWindow, NSWindowDelegate, NSWindowStyleMask,
};
use objc2_foundation::{
    MainThreadMarker, NSArray, NSError, NSObjectProtocol, NSPoint, NSRect, NSSize, NSString,
    NSURLRequest, NSUTF8StringEncoding, NSURL,
};
use objc2_web_kit::{
    WKNavigationAction, WKNavigationActionPolicy, WKNavigationDelegate, WKNavigationType,
    WKWebExtensionContext, WKWebExtensionController, WKWebExtensionTab, WKWebExtensionWindow,
    WKWebView,
};
use zephium_core::extensions::{
    ExtensionBrowserRequestAction, ExtensionBrowserRequestRejection,
    MAX_EXTENSION_BROWSER_REQUEST_URL_BYTES,
};

use crate::host::NativeResourceLease;

use super::browser_request_broker::BrowserRequestBroker;

const EXTENSION_PAGE_WIDTH: f64 = 960.0;
const EXTENSION_PAGE_HEIGHT: f64 = 720.0;

struct ExtensionPageTabIvars {
    broker: RcWeak<ExtensionPageBroker>,
    context: Retained<WKWebExtensionContext>,
    window: RefCell<Option<Weak<ExtensionPageWindow>>>,
    webview: RefCell<Option<Weak<WKWebView>>>,
    title: RefCell<Retained<NSString>>,
    url: RefCell<Retained<NSURL>>,
    loading: Cell<bool>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "ZephiumExtensionPageTab"]
    #[ivars = ExtensionPageTabIvars]
    struct ExtensionPageTab;

    unsafe impl NSObjectProtocol for ExtensionPageTab {}

    unsafe impl WKWebExtensionTab for ExtensionPageTab {
        #[unsafe(method_id(windowForWebExtensionContext:))]
        fn window_for_context(
            &self,
            context: &WKWebExtensionContext,
        ) -> Option<Retained<ProtocolObject<dyn WKWebExtensionWindow>>> {
            self.accepts(context)
                .then(|| self.ivars().window.borrow().as_ref().and_then(Weak::load))
                .flatten()
                .map(ProtocolObject::from_retained)
        }

        #[unsafe(method_id(webViewForWebExtensionContext:))]
        fn webview_for_context(
            &self,
            context: &WKWebExtensionContext,
        ) -> Option<Retained<WKWebView>> {
            self.accepts(context)
                .then(|| self.ivars().webview.borrow().as_ref().and_then(Weak::load))
                .flatten()
        }

        #[unsafe(method_id(titleForWebExtensionContext:))]
        fn title_for_context(&self, context: &WKWebExtensionContext) -> Option<Retained<NSString>> {
            self.accepts(context)
                .then(|| self.ivars().title.borrow().clone())
        }

        #[unsafe(method_id(urlForWebExtensionContext:))]
        fn url_for_context(&self, context: &WKWebExtensionContext) -> Option<Retained<NSURL>> {
            self.accepts(context)
                .then(|| self.ivars().url.borrow().clone())
        }

        #[unsafe(method(isLoadingCompleteForWebExtensionContext:))]
        fn is_loading_complete(&self, context: &WKWebExtensionContext) -> bool {
            self.accepts(context) && !self.ivars().loading.get()
        }

        #[unsafe(method(isPinnedForWebExtensionContext:))]
        fn is_pinned(&self, _context: &WKWebExtensionContext) -> bool {
            false
        }

        #[unsafe(method(indexInWindowForWebExtensionContext:))]
        fn index_in_window(&self, _context: &WKWebExtensionContext) -> usize {
            0
        }

        #[unsafe(method(isSelectedForWebExtensionContext:))]
        fn is_selected(&self, context: &WKWebExtensionContext) -> bool {
            self.accepts(context)
        }

        #[unsafe(method(loadURL:forWebExtensionContext:completionHandler:))]
        fn load_url(
            &self,
            url: &NSURL,
            context: &WKWebExtensionContext,
            completion: &DynBlock<dyn Fn(*mut NSError)>,
        ) {
            self.with_broker_or_reject(context, completion, |broker| {
                broker.load(self, context, url, completion)
            });
        }

        #[unsafe(method(activateForWebExtensionContext:completionHandler:))]
        fn activate(
            &self,
            context: &WKWebExtensionContext,
            completion: &DynBlock<dyn Fn(*mut NSError)>,
        ) {
            self.with_broker_or_reject(context, completion, |broker| {
                broker.activate(self, context, completion)
            });
        }

        #[unsafe(method(setSelected:forWebExtensionContext:completionHandler:))]
        fn set_selected(
            &self,
            selected: bool,
            context: &WKWebExtensionContext,
            completion: &DynBlock<dyn Fn(*mut NSError)>,
        ) {
            if selected {
                self.with_broker_or_reject(context, completion, |broker| {
                    broker.activate(self, context, completion)
                });
            } else {
                self.reject(completion, ExtensionBrowserRequestRejection::Unsupported);
            }
        }

        #[unsafe(method(closeForWebExtensionContext:completionHandler:))]
        fn close(
            &self,
            context: &WKWebExtensionContext,
            completion: &DynBlock<dyn Fn(*mut NSError)>,
        ) {
            self.with_broker_or_reject(context, completion, |broker| {
                broker.close(self, context, completion)
            });
        }

        #[unsafe(method(reloadFromOrigin:forWebExtensionContext:completionHandler:))]
        fn reload(
            &self,
            from_origin: bool,
            context: &WKWebExtensionContext,
            completion: &DynBlock<dyn Fn(*mut NSError)>,
        ) {
            if from_origin {
                self.reject(completion, ExtensionBrowserRequestRejection::Unsupported);
                return;
            }
            self.with_broker_or_reject(context, completion, |broker| {
                broker.reload(self, context, completion)
            });
        }

        #[unsafe(method(goBackForWebExtensionContext:completionHandler:))]
        fn go_back(
            &self,
            context: &WKWebExtensionContext,
            completion: &DynBlock<dyn Fn(*mut NSError)>,
        ) {
            self.with_broker_or_reject(context, completion, |broker| {
                broker.traverse(self, context, false, completion)
            });
        }

        #[unsafe(method(goForwardForWebExtensionContext:completionHandler:))]
        fn go_forward(
            &self,
            context: &WKWebExtensionContext,
            completion: &DynBlock<dyn Fn(*mut NSError)>,
        ) {
            self.with_broker_or_reject(context, completion, |broker| {
                broker.traverse(self, context, true, completion)
            });
        }

        #[unsafe(method(setParentTab:forWebExtensionContext:completionHandler:))]
        fn set_parent_tab(
            &self,
            _parent: Option<&ProtocolObject<dyn WKWebExtensionTab>>,
            _context: &WKWebExtensionContext,
            completion: &DynBlock<dyn Fn(*mut NSError)>,
        ) {
            self.reject(completion, ExtensionBrowserRequestRejection::Unsupported);
        }

        #[unsafe(method(setPinned:forWebExtensionContext:completionHandler:))]
        fn set_pinned(
            &self,
            _pinned: bool,
            _context: &WKWebExtensionContext,
            completion: &DynBlock<dyn Fn(*mut NSError)>,
        ) {
            self.reject(completion, ExtensionBrowserRequestRejection::Unsupported);
        }

        #[unsafe(method(setReaderModeActive:forWebExtensionContext:completionHandler:))]
        fn set_reader_mode(
            &self,
            _active: bool,
            _context: &WKWebExtensionContext,
            completion: &DynBlock<dyn Fn(*mut NSError)>,
        ) {
            self.reject(completion, ExtensionBrowserRequestRejection::Unsupported);
        }

        #[unsafe(method(setMuted:forWebExtensionContext:completionHandler:))]
        fn set_muted(
            &self,
            _muted: bool,
            _context: &WKWebExtensionContext,
            completion: &DynBlock<dyn Fn(*mut NSError)>,
        ) {
            self.reject(completion, ExtensionBrowserRequestRejection::Unsupported);
        }

        #[unsafe(method(setZoomFactor:forWebExtensionContext:completionHandler:))]
        fn set_zoom_factor(
            &self,
            _zoom: f64,
            _context: &WKWebExtensionContext,
            completion: &DynBlock<dyn Fn(*mut NSError)>,
        ) {
            self.reject(completion, ExtensionBrowserRequestRejection::Unsupported);
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

impl ExtensionPageTab {
    fn new(
        mtm: MainThreadMarker,
        broker: RcWeak<ExtensionPageBroker>,
        context: Retained<WKWebExtensionContext>,
        title: Retained<NSString>,
        url: Retained<NSURL>,
    ) -> Retained<Self> {
        let object = Self::alloc(mtm).set_ivars(ExtensionPageTabIvars {
            broker,
            context,
            window: RefCell::new(None),
            webview: RefCell::new(None),
            title: RefCell::new(title),
            url: RefCell::new(url),
            loading: Cell::new(true),
        });
        // SAFETY: NSObject is the declared superclass and every ivar is
        // initialized before its initializer runs.
        unsafe { msg_send![super(object), init] }
    }

    fn accepts(&self, context: &WKWebExtensionContext) -> bool {
        std::ptr::eq(&*self.ivars().context, context)
            && self
                .ivars()
                .broker
                .upgrade()
                .is_some_and(|broker| broker.accepts_context(context))
    }

    fn with_broker_or_reject(
        &self,
        context: &WKWebExtensionContext,
        completion: &DynBlock<dyn Fn(*mut NSError)>,
        operation: impl FnOnce(&ExtensionPageBroker),
    ) {
        let Some(broker) = self
            .ivars()
            .broker
            .upgrade()
            .filter(|_| self.accepts(context))
        else {
            self.reject(completion, ExtensionBrowserRequestRejection::InvalidContext);
            return;
        };
        operation(&broker);
    }

    fn reject(
        &self,
        completion: &DynBlock<dyn Fn(*mut NSError)>,
        reason: ExtensionBrowserRequestRejection,
    ) {
        if let Some(broker) = self.ivars().broker.upgrade() {
            broker.browser_requests.reject_unit(completion, reason);
        }
    }
}

struct ExtensionPageWindowIvars {
    broker: RcWeak<ExtensionPageBroker>,
    context: Retained<WKWebExtensionContext>,
    tab: Retained<ExtensionPageTab>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "ZephiumExtensionPageWindow"]
    #[ivars = ExtensionPageWindowIvars]
    struct ExtensionPageWindow;

    unsafe impl NSObjectProtocol for ExtensionPageWindow {}

    unsafe impl WKWebExtensionWindow for ExtensionPageWindow {
        #[unsafe(method_id(tabsForWebExtensionContext:))]
        fn tabs_for_context(
            &self,
            context: &WKWebExtensionContext,
        ) -> Retained<NSArray<ProtocolObject<dyn WKWebExtensionTab>>> {
            let tabs = if self.accepts(context) {
                vec![ProtocolObject::from_retained(self.ivars().tab.clone())]
            } else {
                Vec::new()
            };
            NSArray::from_retained_slice(&tabs)
        }

        #[unsafe(method_id(activeTabForWebExtensionContext:))]
        fn active_tab_for_context(
            &self,
            context: &WKWebExtensionContext,
        ) -> Option<Retained<ProtocolObject<dyn WKWebExtensionTab>>> {
            self.accepts(context)
                .then(|| ProtocolObject::from_retained(self.ivars().tab.clone()))
        }

        #[unsafe(method(isPrivateForWebExtensionContext:))]
        fn is_private(&self, _context: &WKWebExtensionContext) -> bool {
            false
        }
    }
);

impl ExtensionPageWindow {
    fn new(
        mtm: MainThreadMarker,
        broker: RcWeak<ExtensionPageBroker>,
        context: Retained<WKWebExtensionContext>,
        tab: Retained<ExtensionPageTab>,
    ) -> Retained<Self> {
        let object = Self::alloc(mtm).set_ivars(ExtensionPageWindowIvars {
            broker,
            context,
            tab,
        });
        // SAFETY: NSObject is the declared superclass and every ivar is
        // initialized before its initializer runs.
        unsafe { msg_send![super(object), init] }
    }

    fn accepts(&self, context: &WKWebExtensionContext) -> bool {
        std::ptr::eq(&*self.ivars().context, context)
            && self
                .ivars()
                .broker
                .upgrade()
                .is_some_and(|broker| broker.accepts_context(context))
    }
}

struct ExtensionPageDelegateIvars {
    broker: RcWeak<ExtensionPageBroker>,
    browser_requests: Rc<BrowserRequestBroker>,
    context: Retained<WKWebExtensionContext>,
    extension_origin: Box<str>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "ZephiumExtensionPageDelegate"]
    #[ivars = ExtensionPageDelegateIvars]
    struct ExtensionPageDelegate;

    unsafe impl NSObjectProtocol for ExtensionPageDelegate {}

    unsafe impl NSWindowDelegate for ExtensionPageDelegate {
        #[unsafe(method(windowWillClose:))]
        fn window_will_close(&self, _notification: &objc2_foundation::NSNotification) {
            if let Some(broker) = self.ivars().broker.upgrade() {
                broker.schedule_close(false);
            }
        }
    }

    unsafe impl WKNavigationDelegate for ExtensionPageDelegate {
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
            let main_frame =
                unsafe { action.targetFrame() }.is_some_and(|frame| frame.isMainFrame());
            let route = absolute.as_ref().and_then(|absolute| {
                (absolute.lengthOfBytesUsingEncoding(NSUTF8StringEncoding)
                    <= MAX_EXTENSION_BROWSER_REQUEST_URL_BYTES)
                    .then(|| {
                        objc2::rc::autoreleasepool(|pool| {
                            classify_navigation(
                                self.ivars().extension_origin.as_ref(),
                                unsafe { absolute.to_str(pool) },
                                unsafe { action.navigationType() }
                                    == WKNavigationType::LinkActivated,
                                main_frame,
                            )
                        })
                    })
                    .flatten()
            });
            match route {
                Some(ExtensionPageNavigation::Internal) => {
                    decision.call((WKNavigationActionPolicy::Allow,));
                }
                Some(ExtensionPageNavigation::External { replace }) => {
                    if self
                        .ivars()
                        .browser_requests
                        .accepts(None, &self.ivars().context)
                    {
                        if let Ok(url) = super::browser_surface::request_url(&url) {
                            let broker = self.ivars().broker.clone();
                            let completion: RcBlock<
                                dyn Fn(*mut ProtocolObject<dyn WKWebExtensionTab>, *mut NSError),
                            > = RcBlock::new(
                                move |tab: *mut ProtocolObject<dyn WKWebExtensionTab>,
                                      error: *mut NSError| {
                                    if replace && !tab.is_null() && error.is_null() {
                                        if let Some(broker) = broker.upgrade() {
                                            broker.schedule_close(true);
                                        }
                                    }
                                },
                            );
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
                    decision.call((WKNavigationActionPolicy::Cancel,));
                }
                None => decision.call((WKNavigationActionPolicy::Cancel,)),
            }
        }

        #[unsafe(method(webViewWebContentProcessDidTerminate:))]
        fn web_content_process_did_terminate(&self, _webview: &WKWebView) {
            if let Some(broker) = self.ivars().broker.upgrade() {
                broker.schedule_close(true);
            }
        }
    }
);

impl ExtensionPageDelegate {
    fn new(
        mtm: MainThreadMarker,
        broker: RcWeak<ExtensionPageBroker>,
        browser_requests: Rc<BrowserRequestBroker>,
        context: Retained<WKWebExtensionContext>,
        extension_origin: Box<str>,
    ) -> Retained<Self> {
        let object = Self::alloc(mtm).set_ivars(ExtensionPageDelegateIvars {
            broker,
            browser_requests,
            context,
            extension_origin,
        });
        // SAFETY: NSObject is the declared superclass and every ivar is
        // initialized before its initializer runs.
        unsafe { msg_send![super(object), init] }
    }
}

struct ActiveExtensionPage {
    context: Retained<WKWebExtensionContext>,
    native_window: Retained<NSWindow>,
    logical_window: Retained<ExtensionPageWindow>,
    tab: Retained<ExtensionPageTab>,
    webview: Retained<WKWebView>,
    delegate: Retained<ExtensionPageDelegate>,
    _lease: NativeResourceLease,
}

pub(super) struct ExtensionPageBroker {
    browser_requests: Rc<BrowserRequestBroker>,
    controller: RefCell<Option<Weak<WKWebExtensionController>>>,
    active: RefCell<Option<ActiveExtensionPage>>,
    sealed: Cell<bool>,
}

impl ExtensionPageBroker {
    pub(super) fn new(browser_requests: Rc<BrowserRequestBroker>) -> Rc<Self> {
        Rc::new(Self {
            browser_requests,
            controller: RefCell::new(None),
            active: RefCell::new(None),
            sealed: Cell::new(false),
        })
    }

    pub(super) fn bind_controller(&self, controller: &Retained<WKWebExtensionController>) {
        *self.controller.borrow_mut() = Some(Weak::from_retained(controller));
    }

    pub(super) fn accepts_context(&self, context: &WKWebExtensionContext) -> bool {
        if self.sealed.get() {
            return false;
        }
        self.controller
            .borrow()
            .as_ref()
            .and_then(Weak::load)
            .is_some_and(|controller| unsafe {
                controller.extensionContexts().containsObject(context)
                    && context
                        .webExtensionController()
                        .as_ref()
                        .is_some_and(|actual| {
                            Retained::as_ptr(actual) == Retained::as_ptr(&controller)
                        })
            })
    }

    pub(super) fn accepts_url(&self, context: &WKWebExtensionContext, url: &NSURL) -> bool {
        self.accepts_context(context)
            && extension_origin(context)
                .zip(url.absoluteString())
                .is_some_and(|(origin, absolute)| {
                    absolute.lengthOfBytesUsingEncoding(NSUTF8StringEncoding)
                        <= MAX_EXTENSION_BROWSER_REQUEST_URL_BYTES
                        && objc2::rc::autoreleasepool(|pool| {
                            classify_navigation(
                                &origin,
                                unsafe { absolute.to_str(pool) },
                                false,
                                true,
                            ) == Some(ExtensionPageNavigation::Internal)
                        })
                })
    }

    pub(super) fn present(
        self: &Rc<Self>,
        context: Retained<WKWebExtensionContext>,
        url: Retained<NSURL>,
        lease: NativeResourceLease,
    ) -> Result<Retained<ProtocolObject<dyn WKWebExtensionTab>>, ExtensionBrowserRequestRejection>
    {
        if !self.accepts_url(&context, &url) {
            return Err(ExtensionBrowserRequestRejection::InvalidContext);
        }
        if self.active.borrow().is_some() {
            return Err(ExtensionBrowserRequestRejection::CapacityExceeded);
        }
        let origin =
            extension_origin(&context).ok_or(ExtensionBrowserRequestRejection::InvalidRequest)?;
        let configuration = unsafe { context.webViewConfiguration() }
            .ok_or(ExtensionBrowserRequestRejection::NativeAdmissionFailed)?;
        let mtm = MainThreadMarker::new()
            .ok_or(ExtensionBrowserRequestRejection::NativeAdmissionFailed)?;
        let frame = NSRect::new(
            NSPoint::new(0.0, 0.0),
            NSSize::new(EXTENSION_PAGE_WIDTH, EXTENSION_PAGE_HEIGHT),
        );
        let webview = unsafe {
            WKWebView::initWithFrame_configuration(WKWebView::alloc(mtm), frame, &configuration)
        };
        webview.setAutoresizingMask(
            NSAutoresizingMaskOptions::ViewWidthSizable
                | NSAutoresizingMaskOptions::ViewHeightSizable,
        );
        let title = unsafe { context.webExtension().displayName() }
            .unwrap_or_else(|| NSString::from_str("Extension"));
        let tab = ExtensionPageTab::new(
            mtm,
            Rc::downgrade(self),
            context.clone(),
            title.clone(),
            url.clone(),
        );
        let logical_window =
            ExtensionPageWindow::new(mtm, Rc::downgrade(self), context.clone(), tab.clone());
        *tab.ivars().window.borrow_mut() = Some(Weak::from_retained(&logical_window));
        *tab.ivars().webview.borrow_mut() = Some(Weak::from_retained(&webview));
        let delegate = ExtensionPageDelegate::new(
            mtm,
            Rc::downgrade(self),
            Rc::clone(&self.browser_requests),
            context.clone(),
            origin,
        );
        unsafe { webview.setNavigationDelegate(Some(ProtocolObject::from_ref(&*delegate))) };
        let request = NSURLRequest::requestWithURL(&url);
        if unsafe { webview.loadRequest(&request) }.is_none() {
            unsafe { webview.setNavigationDelegate(None) };
            return Err(ExtensionBrowserRequestRejection::NativeAdmissionFailed);
        }

        let native_window = unsafe {
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
            native_window.setReleasedWhenClosed(false);
            native_window.setTitle(&title);
        }
        native_window.setContentView(Some(&webview));
        native_window.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
        *self.active.borrow_mut() = Some(ActiveExtensionPage {
            context,
            native_window: native_window.clone(),
            logical_window: logical_window.clone(),
            tab: tab.clone(),
            webview,
            delegate,
            _lease: lease,
        });

        let Some(controller) = self.controller.borrow().as_ref().and_then(Weak::load) else {
            self.close_active(true);
            return Err(ExtensionBrowserRequestRejection::InvalidContext);
        };
        unsafe {
            controller.didOpenWindow(ProtocolObject::from_ref(&*logical_window));
            controller.didOpenTab(ProtocolObject::from_ref(&*tab));
            controller.didFocusWindow(Some(ProtocolObject::from_ref(&*logical_window)));
        }
        native_window.center();
        native_window.makeKeyAndOrderFront(None);
        tab.ivars().loading.set(false);
        Ok(ProtocolObject::from_retained(tab))
    }

    pub(super) fn open_window(&self) -> Option<Retained<ProtocolObject<dyn WKWebExtensionWindow>>> {
        self.active
            .borrow()
            .as_ref()
            .map(|active| ProtocolObject::from_retained(active.logical_window.clone()))
    }

    pub(super) fn focused_window(
        &self,
    ) -> Option<Retained<ProtocolObject<dyn WKWebExtensionWindow>>> {
        self.active.borrow().as_ref().and_then(|active| {
            active
                .native_window
                .isKeyWindow()
                .then(|| ProtocolObject::from_retained(active.logical_window.clone()))
        })
    }

    pub(super) fn contains_window(
        &self,
        requested: &ProtocolObject<dyn WKWebExtensionWindow>,
    ) -> bool {
        self.active
            .borrow()
            .as_ref()
            .is_some_and(|active| ProtocolObject::from_ref(&*active.logical_window) == requested)
    }

    pub(super) fn cancel_context(&self, context: *const WKWebExtensionContext) {
        if self
            .active
            .borrow()
            .as_ref()
            .is_some_and(|active| std::ptr::eq(Retained::as_ptr(&active.context), context))
        {
            self.close_active(true);
        }
    }

    pub(super) fn seal_and_close(&self) {
        self.sealed.set(true);
        self.close_active(true);
        self.controller.borrow_mut().take();
    }

    fn matches(&self, tab: &ExtensionPageTab, context: &WKWebExtensionContext) -> bool {
        self.active.borrow().as_ref().is_some_and(|active| {
            std::ptr::eq(&*active.tab, tab)
                && std::ptr::eq(&*active.context, context)
                && self.accepts_context(context)
        })
    }

    fn load(
        &self,
        tab: &ExtensionPageTab,
        context: &WKWebExtensionContext,
        url: &NSURL,
        completion: &DynBlock<dyn Fn(*mut NSError)>,
    ) {
        if !self.matches(tab, context) || !self.accepts_url(context, url) {
            self.browser_requests
                .reject_unit(completion, ExtensionBrowserRequestRejection::InvalidRequest);
            return;
        }
        let loaded = self.active.borrow().as_ref().is_some_and(|active| {
            unsafe {
                active
                    .webview
                    .loadRequest(&NSURLRequest::requestWithURL(url))
            }
            .is_some()
        });
        if loaded {
            let Some(url) = (unsafe { Retained::retain(url as *const _ as *mut _) }) else {
                self.browser_requests.reject_unit(
                    completion,
                    ExtensionBrowserRequestRejection::NativeAdmissionFailed,
                );
                return;
            };
            *tab.ivars().url.borrow_mut() = url;
            completion.call((std::ptr::null_mut(),));
        } else {
            self.browser_requests.reject_unit(
                completion,
                ExtensionBrowserRequestRejection::NativeAdmissionFailed,
            );
        }
    }

    fn activate(
        &self,
        tab: &ExtensionPageTab,
        context: &WKWebExtensionContext,
        completion: &DynBlock<dyn Fn(*mut NSError)>,
    ) {
        if let Some(window) = self
            .active
            .borrow()
            .as_ref()
            .filter(|_| self.matches(tab, context))
            .map(|active| active.native_window.clone())
        {
            window.makeKeyAndOrderFront(None);
            completion.call((std::ptr::null_mut(),));
        } else {
            self.browser_requests
                .reject_unit(completion, ExtensionBrowserRequestRejection::InvalidContext);
        }
    }

    fn close(
        &self,
        tab: &ExtensionPageTab,
        context: &WKWebExtensionContext,
        completion: &DynBlock<dyn Fn(*mut NSError)>,
    ) {
        if !self.matches(tab, context) {
            self.browser_requests
                .reject_unit(completion, ExtensionBrowserRequestRejection::InvalidContext);
            return;
        }
        self.close_active(true);
        completion.call((std::ptr::null_mut(),));
    }

    fn reload(
        &self,
        tab: &ExtensionPageTab,
        context: &WKWebExtensionContext,
        completion: &DynBlock<dyn Fn(*mut NSError)>,
    ) {
        if let Some(webview) = self
            .active
            .borrow()
            .as_ref()
            .filter(|_| self.matches(tab, context))
            .map(|active| active.webview.clone())
        {
            unsafe { webview.reload() };
            completion.call((std::ptr::null_mut(),));
        } else {
            self.browser_requests
                .reject_unit(completion, ExtensionBrowserRequestRejection::InvalidContext);
        }
    }

    fn traverse(
        &self,
        tab: &ExtensionPageTab,
        context: &WKWebExtensionContext,
        forward: bool,
        completion: &DynBlock<dyn Fn(*mut NSError)>,
    ) {
        let navigated = self.active.borrow().as_ref().is_some_and(|active| {
            if !self.matches(tab, context) {
                return false;
            }
            if forward {
                unsafe { active.webview.goForward() }.is_some()
            } else {
                unsafe { active.webview.goBack() }.is_some()
            }
        });
        if navigated {
            completion.call((std::ptr::null_mut(),));
        } else {
            self.browser_requests
                .reject_unit(completion, ExtensionBrowserRequestRejection::InvalidRequest);
        }
    }

    fn schedule_close(self: &Rc<Self>, close_window: bool) {
        let broker = Rc::downgrade(self);
        let completion: RcBlock<dyn Fn()> = RcBlock::new(move || {
            if let Some(broker) = broker.upgrade() {
                broker.close_active(close_window);
            }
        });
        // SAFETY: the main queue copies this block. Deferring keeps the
        // Objective-C delegate alive until WebKit/AppKit returns from the
        // callback which initiated teardown.
        unsafe {
            dispatch2::DispatchQueue::main().exec_async_with_block(RcBlock::as_ptr(&completion));
        }
    }

    fn close_active(&self, close_window: bool) {
        let Some(active) = self.active.borrow_mut().take() else {
            return;
        };
        if let Some(controller) = self.controller.borrow().as_ref().and_then(Weak::load) {
            unsafe {
                controller
                    .didCloseTab_windowIsClosing(ProtocolObject::from_ref(&*active.tab), true);
                controller.didCloseWindow(ProtocolObject::from_ref(&*active.logical_window));
                controller.didFocusWindow(None);
            }
        }
        active.native_window.setDelegate(None);
        unsafe {
            active.webview.setNavigationDelegate(None);
            active.webview.stopLoading();
        }
        active.native_window.setContentView(None);
        active.native_window.orderOut(None);
        if close_window {
            active.native_window.close();
        }
        let _keep_delegate_alive_through_teardown = active.delegate;
    }
}

impl Drop for ExtensionPageBroker {
    fn drop(&mut self) {
        self.seal_and_close();
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ExtensionPageNavigation {
    Internal,
    External { replace: bool },
}

fn extension_origin(context: &WKWebExtensionContext) -> Option<Box<str>> {
    let base = unsafe { context.baseURL() };
    let absolute = base.absoluteString()?;
    if absolute.lengthOfBytesUsingEncoding(NSUTF8StringEncoding)
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
        Some(format!("webkit-extension://{}/", parsed.host_str()?).into_boxed_str())
    })
}

fn classify_navigation(
    extension_origin: &str,
    requested: &str,
    link_activated: bool,
    main_frame: bool,
) -> Option<ExtensionPageNavigation> {
    if requested.len() > MAX_EXTENSION_BROWSER_REQUEST_URL_BYTES {
        return None;
    }
    if requested.starts_with(extension_origin) {
        return Some(ExtensionPageNavigation::Internal);
    }
    ((link_activated || main_frame) && zephium_core::navigation::is_allowed_str(requested))
        .then_some(ExtensionPageNavigation::External {
            replace: main_frame && !link_activated,
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extension_page_navigation_is_principal_exact() {
        let origin = "webkit-extension://aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa/";
        assert_eq!(
            classify_navigation(
                origin,
                "webkit-extension://aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa/app/app.html#/welcome",
                false,
                true,
            ),
            Some(ExtensionPageNavigation::Internal)
        );
        assert_eq!(
            classify_navigation(origin, "https://example.com/help", true, true),
            Some(ExtensionPageNavigation::External { replace: false })
        );
        assert_eq!(
            classify_navigation(origin, "https://example.com/sign-in", false, true),
            Some(ExtensionPageNavigation::External { replace: true })
        );
        for (url, gesture, main_frame) in [
            ("https://example.com/iframe", false, false),
            ("javascript:alert(1)", true, true),
            ("file:///tmp/private", true, true),
            (
                "webkit-extension://bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb/app/app.html",
                true,
                true,
            ),
        ] {
            assert_eq!(classify_navigation(origin, url, gesture, main_frame), None);
        }
    }

    #[test]
    fn extension_page_protocol_objects_remain_main_thread_only() {
        fn assert_main_thread_only<T: MainThreadOnly>() {}
        assert_main_thread_only::<ExtensionPageTab>();
        assert_main_thread_only::<ExtensionPageWindow>();
        assert_main_thread_only::<ExtensionPageDelegate>();
    }
}
