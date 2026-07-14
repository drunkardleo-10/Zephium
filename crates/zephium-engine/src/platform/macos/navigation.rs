use std::ffi::c_void;
use std::ptr::null_mut;

use objc2::rc::{Retained, Weak};
use objc2::runtime::{AnyObject, NSObject};
use objc2::{define_class, msg_send, DefinedClass, MainThreadOnly};
use objc2_foundation::{
    ns_string, MainThreadMarker, NSDictionary, NSKeyValueChangeKey, NSKeyValueObservingOptions,
    NSObjectNSKeyValueObserverRegistration, NSObjectProtocol, NSString,
};
use objc2_web_kit::WKWebView;

pub struct NavigationObserverIvars {
    // The host retains both this observer and the Wry WebView. Keeping only a
    // zeroing weak reference here prevents observer -> WKWebView retention and
    // lets the host tear both objects down deterministically.
    webview: Weak<WKWebView>,
    on_change: Box<dyn Fn()>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "ZephiumNavigationObserver"]
    #[ivars = NavigationObserverIvars]
    pub struct NavigationObserver;

    impl NavigationObserver {
        #[unsafe(method(observeValueForKeyPath:ofObject:change:context:))]
        fn observe_value_for_key_path(
            &self,
            key_path: Option<&NSString>,
            _object: Option<&AnyObject>,
            _change: Option<&NSDictionary<NSKeyValueChangeKey, AnyObject>>,
            _context: *mut c_void,
        ) {
            let Some(key_path) = key_path else {
                return;
            };
            if key_path.isEqualToString(ns_string!("URL"))
                || key_path.isEqualToString(ns_string!("canGoBack"))
                || key_path.isEqualToString(ns_string!("canGoForward"))
            {
                (self.ivars().on_change)();
            }
        }
    }

    unsafe impl NSObjectProtocol for NavigationObserver {}
);

impl NavigationObserver {
    pub fn install(
        mtm: MainThreadMarker,
        webview: &Retained<WKWebView>,
        on_change: impl Fn() + 'static,
    ) -> Retained<Self> {
        let observer = Self::alloc(mtm).set_ivars(NavigationObserverIvars {
            webview: Weak::from_retained(webview),
            on_change: Box::new(on_change),
        });
        let observer: Retained<Self> = unsafe { msg_send![super(observer), init] };

        // WKWebView documents all three properties as KVO-compliant. Do not
        // request Initial while the newly built view is not yet in the host
        // map; its first source change (or page-load fallback) snapshots it.
        unsafe {
            for key in [
                ns_string!("URL"),
                ns_string!("canGoBack"),
                ns_string!("canGoForward"),
            ] {
                webview.addObserver_forKeyPath_options_context(
                    &observer,
                    key,
                    NSKeyValueObservingOptions::New,
                    null_mut(),
                );
            }
        }
        observer
    }
}

impl Drop for NavigationObserver {
    fn drop(&mut self) {
        // ObservedView drops this field before its WebView field, so the weak
        // upgrade normally succeeds. It can legitimately fail if native
        // teardown already deallocated the view; zeroing weak references make
        // that case a safe no-op.
        let Some(webview) = self.ivars().webview.load() else {
            return;
        };
        unsafe {
            for key in [
                ns_string!("URL"),
                ns_string!("canGoBack"),
                ns_string!("canGoForward"),
            ] {
                webview.removeObserver_forKeyPath(self, key);
            }
        }
    }
}
