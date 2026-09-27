//! Same-principal extension documents embedded in browser-owned tab slots.
use super::browser_request_broker::BrowserRequestBroker;
use crate::platform::imp::ContentStage;
use block2::{DynBlock, RcBlock};
use objc2::rc::{Retained, Weak};
use objc2::runtime::{AnyObject, NSObject, ProtocolObject};
use objc2::{define_class, DefinedClass, MainThreadOnly};
use objc2_foundation::{
    ns_string, NSDictionary, NSError, NSKeyValueChangeKey, NSObjectNSKeyValueObserverRegistration,
    NSObjectProtocol, NSString, NSURLRequest, NSUTF8StringEncoding, NSURL,
};
use objc2_web_kit::{
    WKNavigation, WKNavigationAction, WKNavigationActionPolicy, WKNavigationDelegate,
    WKNavigationType, WKUIDelegate, WKWebExtensionContext, WKWebExtensionController,
    WKWebExtensionTab, WKWebView, WKWebViewConfiguration, WKWindowFeatures,
};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::ffi::c_void;
use std::rc::{Rc, Weak as RcWeak};
#[cfg(feature = "native-web-extension-probes")]
use std::sync::atomic::AtomicUsize;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use zephium_core::extensions::{
    ExtensionBrowserRequestAction, ExtensionBrowserRequestRejection,
    MAX_EXTENSION_BROWSER_REQUEST_URL_BYTES,
};
use zephium_core::ids::ItemId;

#[cfg(feature = "native-web-extension-probes")]
static PROBE_NEW_WINDOW_CALLBACKS: AtomicUsize = AtomicUsize::new(0);
#[cfg(feature = "native-web-extension-probes")]
static PROBE_NEW_WINDOW_POLICY_ATTEMPTS: AtomicUsize = AtomicUsize::new(0);
#[cfg(feature = "native-web-extension-probes")]
static PROBE_NEW_WINDOW_POLICY_ALLOWED: AtomicUsize = AtomicUsize::new(0);

#[cfg(feature = "native-web-extension-probes")]
pub(crate) fn probe_new_window_callbacks() -> usize {
    PROBE_NEW_WINDOW_CALLBACKS.load(Ordering::Acquire)
}

#[cfg(feature = "native-web-extension-probes")]
pub(crate) fn probe_new_window_policy() -> (usize, usize) {
    (
        PROBE_NEW_WINDOW_POLICY_ATTEMPTS.load(Ordering::Acquire),
        PROBE_NEW_WINDOW_POLICY_ALLOWED.load(Ordering::Acquire),
    )
}

struct ExtensionPageDelegateIvars {
    broker: RcWeak<ExtensionPageBroker>,
    browser_requests: Rc<BrowserRequestBroker>,
    context: Retained<WKWebExtensionContext>,
    extension_origin: Box<str>,
    item: Box<ItemId>,
    last_new_window_action: RefCell<Option<Retained<WKNavigationAction>>>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "ZephiumExtensionPageDelegate"]
    #[ivars = ExtensionPageDelegateIvars]
    struct ExtensionPageDelegate;

    unsafe impl NSObjectProtocol for ExtensionPageDelegate {}

    unsafe impl WKNavigationDelegate for ExtensionPageDelegate {
        #[unsafe(method(webView:decidePolicyForNavigationAction:decisionHandler:))]
        unsafe fn decide_navigation(
            &self,
            webview: &WKWebView,
            action: &WKNavigationAction,
            decision: &DynBlock<dyn Fn(WKNavigationActionPolicy)>,
        ) {
            if !self
                .ivars()
                .broker
                .upgrade()
                .and_then(|broker| broker.view(*self.ivars().item, &self.ivars().context))
                .is_some_and(|owned| std::ptr::eq(&*owned, webview))
            {
                decision.call((WKNavigationActionPolicy::Cancel,));
                return;
            }
            let Some(url) = action.request().URL() else {
                decision.call((WKNavigationActionPolicy::Cancel,));
                return;
            };
            // WebKit asks WKUIDelegate to create a view for a nil target
            // frame. Admit only a same-principal source and a bounded target,
            // then let that delegate issue the single Shell-owned tab request.
            // Returning Allow here never creates or inherits a WebView.
            let target_frame = unsafe { action.targetFrame() };
            if target_frame.is_none() {
                #[cfg(feature = "native-web-extension-probes")]
                PROBE_NEW_WINDOW_POLICY_ATTEMPTS.fetch_add(1, Ordering::AcqRel);
                decision.call((WKNavigationActionPolicy::Cancel,));
                let _ = self.route_new_window(webview, action);
                return;
            }
            let absolute = url.absoluteString();
            let main_frame = target_frame.is_some_and(|frame| frame.isMainFrame());
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
                            let item = *self.ivars().item;
                            let completion: RcBlock<
                                dyn Fn(*mut ProtocolObject<dyn WKWebExtensionTab>, *mut NSError),
                            > = RcBlock::new(
                                move |tab: *mut ProtocolObject<dyn WKWebExtensionTab>,
                                      error: *mut NSError| {
                                    if replace && !tab.is_null() && error.is_null() {
                                        if let Some(broker) = broker.upgrade() {
                                            broker.schedule_close(item);
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

        #[unsafe(method(webView:didStartProvisionalNavigation:))]
        fn started(&self, webview: &WKWebView, _navigation: Option<&WKNavigation>) {
            if let Some(broker) = self.ivars().broker.upgrade() {
                broker.changed(*self.ivars().item, webview);
            }
        }
        #[unsafe(method(webView:didFinishNavigation:))]
        fn finished(&self, webview: &WKWebView, _navigation: Option<&WKNavigation>) {
            if let Some(broker) = self.ivars().broker.upgrade() {
                broker.changed(*self.ivars().item, webview);
            }
        }
        #[unsafe(method(webView:didFailProvisionalNavigation:withError:))]
        fn provisional_failed(
            &self,
            webview: &WKWebView,
            _navigation: Option<&WKNavigation>,
            _error: &NSError,
        ) {
            if let Some(broker) = self.ivars().broker.upgrade() {
                broker.changed(*self.ivars().item, webview);
            }
        }
        #[unsafe(method(webView:didFailNavigation:withError:))]
        fn failed(
            &self,
            webview: &WKWebView,
            _navigation: Option<&WKNavigation>,
            _error: &NSError,
        ) {
            if let Some(broker) = self.ivars().broker.upgrade() {
                broker.changed(*self.ivars().item, webview);
            }
        }

        #[unsafe(method(webViewWebContentProcessDidTerminate:))]
        fn web_content_process_did_terminate(&self, _webview: &WKWebView) {
            if let Some(broker) = self.ivars().broker.upgrade() {
                broker.schedule_close(*self.ivars().item);
            }
        }
    }

    unsafe impl WKUIDelegate for ExtensionPageDelegate {
        #[unsafe(method_id(webView:createWebViewWithConfiguration:forNavigationAction:windowFeatures:))]
        unsafe fn create_new_view(
            &self,
            webview: &WKWebView,
            _configuration: &WKWebViewConfiguration,
            action: &WKNavigationAction,
            _features: &WKWindowFeatures,
        ) -> Option<Retained<WKWebView>> {
            #[cfg(feature = "native-web-extension-probes")]
            PROBE_NEW_WINDOW_CALLBACKS.fetch_add(1, Ordering::AcqRel);
            // Always let the browser admit a fresh tab configuration.
            let _ = self.route_new_window(webview, action);
            None
        }
    }
);

impl ExtensionPageDelegate {
    fn route_new_window(&self, webview: &WKWebView, action: &WKNavigationAction) -> bool {
        let Some(broker) = self.ivars().broker.upgrade() else {
            return false;
        };
        let live = broker
            .view(*self.ivars().item, &self.ivars().context)
            .is_some_and(|owned| std::ptr::eq(&*owned, webview));
        if !live
            || !self
                .ivars()
                .browser_requests
                .accepts(None, &self.ivars().context)
            || unsafe { action.targetFrame() }.is_some()
            || !source_frame_is_owned(action, self.ivars().extension_origin.as_ref())
        {
            return false;
        }
        let Some(url) = (unsafe { action.request() }).URL() else {
            return false;
        };
        let Some(absolute) = url.absoluteString() else {
            return false;
        };
        if absolute.lengthOfBytesUsingEncoding(NSUTF8StringEncoding)
            > MAX_EXTENSION_BROWSER_REQUEST_URL_BYTES
        {
            return false;
        }
        let route = objc2::rc::autoreleasepool(|pool| {
            classify_new_window_navigation(self.ivars().extension_origin.as_ref(), unsafe {
                absolute.to_str(pool)
            })
        });
        let Some(route) = route else {
            return false;
        };
        let Some(action_owner) = (unsafe { Retained::retain(action as *const _ as *mut _) }) else {
            return false;
        };
        {
            let mut last = self.ivars().last_new_window_action.borrow_mut();
            if last
                .as_ref()
                .is_some_and(|previous| std::ptr::eq(&**previous, action))
            {
                return false;
            }
            *last = Some(action_owner);
        }
        #[cfg(feature = "native-web-extension-probes")]
        PROBE_NEW_WINDOW_POLICY_ALLOWED.fetch_add(1, Ordering::AcqRel);
        let completion: RcBlock<dyn Fn(*mut ProtocolObject<dyn WKWebExtensionTab>, *mut NSError)> =
            RcBlock::new(|_, _| {});
        match route {
            ExtensionPageNavigation::Internal => {
                self.ivars()
                    .browser_requests
                    .begin_extension_page(&completion);
            }
            ExtensionPageNavigation::External { .. } => {
                let Ok(url) = super::browser_surface::request_url(&url) else {
                    return false;
                };
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
        true
    }
}

struct ActiveExtensionPage {
    context: Retained<WKWebExtensionContext>,
    stage: Retained<ContentStage>,
    webview: Retained<WKWebView>,
    _delegate: Retained<ExtensionPageDelegate>,
    metadata_observer: Retained<ExtensionPageMetadataObserver>,
    permit: Arc<AtomicBool>,
    last_metadata: Option<ExtensionPageMetadata>,
}

#[derive(Clone, Eq, PartialEq)]
struct ExtensionPageMetadata {
    title: String,
    loading: bool,
    can_go_back: bool,
    can_go_forward: bool,
}

struct ExtensionPageMetadataObserverIvars {
    broker: RcWeak<ExtensionPageBroker>,
    webview: Weak<WKWebView>,
    item: Box<ItemId>,
    installed: Cell<bool>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "ZephiumExtensionPageMetadataObserver"]
    #[ivars = ExtensionPageMetadataObserverIvars]
    struct ExtensionPageMetadataObserver;

    impl ExtensionPageMetadataObserver {
        #[unsafe(method(observeValueForKeyPath:ofObject:change:context:))]
        fn observed(
            &self,
            key_path: Option<&NSString>,
            object: Option<&AnyObject>,
            _change: Option<&NSDictionary<NSKeyValueChangeKey, AnyObject>>,
            _context: *mut c_void,
        ) {
            if !self.ivars().installed.get()
                || !key_path.is_some_and(|key| metadata_keys().iter().any(|candidate| key.isEqualToString(candidate)))
            {
                return;
            }
            let Some(view) = self.ivars().webview.load() else { return; };
            if !object.is_some_and(|object| std::ptr::eq(object as *const AnyObject, Retained::as_ptr(&view).cast())) {
                return;
            }
            if let Some(broker) = self.ivars().broker.upgrade() {
                broker.changed(*self.ivars().item, &view);
            }
        }
    }

    unsafe impl NSObjectProtocol for ExtensionPageMetadataObserver {}
);

fn metadata_keys() -> [&'static NSString; 4] {
    [
        ns_string!("title"),
        ns_string!("loading"),
        ns_string!("canGoBack"),
        ns_string!("canGoForward"),
    ]
}

impl ExtensionPageMetadataObserver {
    fn remove(&self) {
        if !self.ivars().installed.replace(false) {
            return;
        }
        let Some(view) = self.ivars().webview.load() else {
            return;
        };
        unsafe {
            for key in metadata_keys() {
                view.removeObserver_forKeyPath(self, key);
            }
        }
    }
}

impl Drop for ExtensionPageMetadataObserver {
    fn drop(&mut self) {
        self.remove();
    }
}

pub(super) struct ExtensionPageBroker {
    browser_requests: Rc<BrowserRequestBroker>,
    controller: RefCell<Option<Weak<WKWebExtensionController>>>,
    active: RefCell<HashMap<ItemId, ActiveExtensionPage>>,
    sealed: Cell<bool>,
}

impl ExtensionPageBroker {
    pub(super) fn new(browser_requests: Rc<BrowserRequestBroker>) -> Rc<Self> {
        Rc::new(Self {
            browser_requests,
            controller: RefCell::new(None),
            active: RefCell::new(HashMap::new()),
            sealed: Cell::new(false),
        })
    }
    pub(super) fn bind_controller(&self, controller: &Retained<WKWebExtensionController>) {
        *self.controller.borrow_mut() = Some(Weak::from_retained(controller));
    }
    pub(super) fn accepts_context(&self, context: &WKWebExtensionContext) -> bool {
        !self.sealed.get()
            && self
                .controller
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
    fn changed(&self, item: ItemId, webview: &WKWebView) {
        let Some(context) = self
            .active
            .borrow()
            .get(&item)
            .filter(|page| {
                page.permit.load(Ordering::Acquire) && std::ptr::eq(&*page.webview, webview)
            })
            .map(|page| page.context.clone())
        else {
            return;
        };
        let title = unsafe { webview.title() }
            .filter(|title| {
                let utf16_units = title.length();
                utf16_units > 0 && utf16_units <= 4 * zephium_core::item::MAX_PAGE_TITLE_CHARS
            })
            .or_else(|| unsafe { context.webExtension().displayName() })
            .filter(|title| {
                title.length() <= 4 * zephium_core::item::MAX_PAGE_TITLE_CHARS
                    && title.lengthOfBytesUsingEncoding(NSUTF8StringEncoding)
                        <= 4 * zephium_core::item::MAX_PAGE_TITLE_CHARS
            })
            .map(|title| title.to_string())
            .unwrap_or_else(|| "Extension".into());
        if !self
            .view(item, &context)
            .is_some_and(|owned| std::ptr::eq(&*owned, webview))
        {
            return;
        }
        let metadata = ExtensionPageMetadata {
            title,
            loading: unsafe { webview.isLoading() },
            can_go_back: unsafe { webview.canGoBack() },
            can_go_forward: unsafe { webview.canGoForward() },
        };
        let changed = {
            let mut active = self.active.borrow_mut();
            let Some(page) = active.get_mut(&item).filter(|page| {
                page.permit.load(Ordering::Acquire)
                    && std::ptr::eq(&*page.context, &*context)
                    && std::ptr::eq(&*page.webview, webview)
            }) else {
                return;
            };
            if page.last_metadata.as_ref() == Some(&metadata) {
                false
            } else {
                page.last_metadata = Some(metadata.clone());
                true
            }
        };
        if changed
            && self
                .view(item, &context)
                .is_some_and(|owned| std::ptr::eq(&*owned, webview))
        {
            self.browser_requests.extension_page_changed(
                item,
                metadata.title,
                metadata.loading,
                metadata.can_go_back,
                metadata.can_go_forward,
            );
        }
    }
    pub(super) fn binding(
        &self,
        item: ItemId,
    ) -> Option<(Retained<WKWebExtensionContext>, Retained<WKWebView>)> {
        let binding = self
            .active
            .borrow()
            .get(&item)
            .map(|page| (page.context.clone(), page.webview.clone()));
        binding.filter(|(context, _)| self.accepts_context(context))
    }
    pub(super) fn view(
        &self,
        item: ItemId,
        context: &WKWebExtensionContext,
    ) -> Option<Retained<WKWebView>> {
        if !self.accepts_context(context) {
            return None;
        }
        self.active
            .borrow()
            .get(&item)
            .filter(|page| {
                page.permit.load(Ordering::Acquire) && std::ptr::eq(&*page.context, context)
            })
            .map(|page| page.webview.clone())
    }
    pub(super) fn load(
        &self,
        item: ItemId,
        context: &WKWebExtensionContext,
        url: &NSURL,
        completion: &DynBlock<dyn Fn(*mut NSError)>,
    ) {
        let loaded = self.accepts_url(context, url)
            && self.view(item, context).is_some_and(|view| {
                unsafe { view.loadRequest(&NSURLRequest::requestWithURL(url)) }.is_some()
            });
        if loaded {
            completion.call((std::ptr::null_mut(),));
        } else {
            self.browser_requests
                .reject_unit(completion, ExtensionBrowserRequestRejection::InvalidRequest);
        }
    }
    pub(super) fn close_item(&self, item: ItemId) -> bool {
        let Some(page) = self.active.borrow_mut().remove(&item) else {
            return false;
        };
        page.permit.store(false, Ordering::Release);
        page.metadata_observer.remove();
        unsafe {
            page.webview.setNavigationDelegate(None);
            page.webview.setUIDelegate(None);
            page.webview.stopLoading();
        }
        page.stage.remove_view(item);
        self.browser_requests.extension_page_closed(item);
        true
    }
    pub(super) fn cancel_context(&self, context: *const WKWebExtensionContext) {
        let items: Vec<_> = self
            .active
            .borrow()
            .iter()
            .filter(|(_, page)| std::ptr::eq(Retained::as_ptr(&page.context), context))
            .map(|(id, _)| *id)
            .collect();
        for item in items {
            self.close_item(item);
        }
    }
    pub(super) fn seal_and_close(&self) {
        self.sealed.set(true);
        let items: Vec<_> = self.active.borrow().keys().copied().collect();
        for item in items {
            self.close_item(item);
        }
        self.controller.borrow_mut().take();
    }
    fn schedule_close(self: &Rc<Self>, item: ItemId) {
        let broker = Rc::downgrade(self);
        let completion: RcBlock<dyn Fn()> = RcBlock::new(move || {
            if let Some(broker) = broker.upgrade() {
                broker.close_item(item);
            }
        });
        unsafe {
            dispatch2::DispatchQueue::main().exec_async_with_block(RcBlock::as_ptr(&completion));
        }
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

fn classify_new_window_navigation(
    extension_origin: &str,
    requested: &str,
) -> Option<ExtensionPageNavigation> {
    if requested.len() > MAX_EXTENSION_BROWSER_REQUEST_URL_BYTES {
        return None;
    }
    if requested.starts_with(extension_origin) {
        return Some(ExtensionPageNavigation::Internal);
    }
    let url = url::Url::parse(requested).ok()?;
    (matches!(url.scheme(), "http" | "https")
        && url.username().is_empty()
        && url.password().is_none()
        && zephium_core::navigation::is_allowed(&url))
    .then_some(ExtensionPageNavigation::External { replace: false })
}

fn source_frame_is_owned(action: &WKNavigationAction, extension_origin: &str) -> bool {
    let Some(frame) = (unsafe { action.sourceFrame() }) else {
        return false;
    };
    let Ok(expected) = url::Url::parse(extension_origin) else {
        return false;
    };
    let Some(expected_host) = expected.host_str() else {
        return false;
    };
    // A sandboxed or cross-origin subframe may retain a request URL that
    // looks like our extension URL while its actual security origin is opaque
    // or foreign. Only WebKit's frame origin can authorize opening a browser
    // tab on the extension's behalf.
    let origin = unsafe { frame.securityOrigin() };
    if unsafe { origin.protocol() }.to_string() != "webkit-extension"
        || unsafe { origin.host() }.to_string() != expected_host
        || unsafe { origin.port() } != 0
    {
        return false;
    }
    let Some(url) = unsafe { frame.request() }.URL() else {
        return false;
    };
    let Some(absolute) = url.absoluteString() else {
        return false;
    };
    absolute.lengthOfBytesUsingEncoding(NSUTF8StringEncoding)
        <= MAX_EXTENSION_BROWSER_REQUEST_URL_BYTES
        && objc2::rc::autoreleasepool(|pool| {
            let Ok(parsed) = url::Url::parse(unsafe { absolute.to_str(pool) }) else {
                return false;
            };
            parsed.scheme() == "webkit-extension"
                && parsed.username().is_empty()
                && parsed.password().is_none()
                && parsed.port().is_none()
                && parsed
                    .host_str()
                    .is_some_and(|host| format!("webkit-extension://{host}/") == extension_origin)
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
    fn new_window_target_is_exact_principal_or_safe_web_url() {
        let origin = "webkit-extension://aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa/";
        assert_eq!(
            classify_new_window_navigation(origin, &format!("{origin}pages/command_listing.html")),
            Some(ExtensionPageNavigation::Internal),
        );
        assert_eq!(
            classify_new_window_navigation(origin, "https://example.com/help"),
            Some(ExtensionPageNavigation::External { replace: false }),
        );
        for target in [
            "webkit-extension://bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb/private.html",
            "file:///tmp/private",
            "javascript:alert(1)",
            "https://user:secret@example.com/",
        ] {
            assert_eq!(classify_new_window_navigation(origin, target), None);
        }
    }

    #[test]
    fn extension_page_delegate_remains_main_thread_only() {
        fn assert_main_thread_only<T: MainThreadOnly>() {}
        assert_main_thread_only::<ExtensionPageDelegate>();
    }
}
