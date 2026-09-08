use std::ffi::c_void;
use std::panic::AssertUnwindSafe;
use std::ptr::null_mut;

use objc2::rc::{Retained, Weak};
use objc2::runtime::{AnyObject, NSObject};
use objc2::{define_class, msg_send, DefinedClass, MainThreadOnly};
use objc2_foundation::{
    ns_string, MainThreadMarker, NSDictionary, NSKeyValueChangeKey, NSKeyValueObservingOptions,
    NSObjectNSKeyValueObserverRegistration, NSObjectProtocol, NSString,
};
use objc2_web_kit::{WKContentWorld, WKWebView};

/// Notify isolated content worlds after the host has accepted a changed URL
/// inside the exact committed main-frame epoch.
///
/// The default client world keeps page JavaScript from replacing `Event` or
/// `dispatchEvent`. The resulting DOM event remains visible to every content
/// world, so this is emitted for all accepted same-document URL changes rather
/// than only when an extension is installed; page content cannot use it to
/// enumerate extensions.
pub(crate) fn signal_same_document_navigation(view: &wry::WebView) -> Result<(), &'static str> {
    let mtm = MainThreadMarker::new()
        .ok_or("same-document navigation signaling requires the main thread")?;
    let webview = super::native::webkit(view);
    // SAFETY: `mtm` proves WebKit main-thread affinity. The singleton client
    // world and constant JavaScript source remain valid for the duration of
    // this call; a nil frame selects the main frame.
    let client_world = unsafe { WKContentWorld::defaultClientWorld(mtm) };
    // `ns_string!` places the immutable source in the binary; the route does
    // not allocate or interpolate page data for each History API mutation.
    let source = ns_string!(
        "(()=>{\"use strict\";const d=globalThis.document;if(d!=null){d.dispatchEvent(new globalThis.Event(\"zephium-webkit-same-document-navigation-v1\"));}})();"
    );
    objc2::exception::catch(AssertUnwindSafe(|| unsafe {
        webview.evaluateJavaScript_inFrame_inContentWorld_completionHandler(
            source,
            None,
            &client_world,
            None,
        );
    }))
    .map_err(|_| "WKWebView refused the same-document navigation signal")
}

pub struct NavigationObserverIvars {
    // The host retains both this observer and the Wry WebView. Keeping only a
    // zeroing weak reference here prevents observer -> WKWebView retention and
    // lets the host tear both objects down deterministically.
    webview: Weak<WKWebView>,
    on_change: Box<dyn Fn(NavigationObservation)>,
}

/// Closed native signals emitted by the WKWebView chrome observer.
///
/// `URL` carries one bounded native value sample as evidence. It never grants
/// document authority by itself. `None` means WebKit did not expose a value or
/// the value crossed the native-to-Rust allocation ceiling. Back/forward
/// availability only asks ordinary browser chrome to refresh; it does not
/// prove that the current document or its serialized URL changed.
pub enum NavigationObservation {
    Url(Option<String>),
    HistoryAvailability,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum NavigationObservationKind {
    Url,
    HistoryAvailability,
}

impl NavigationObservationKind {
    fn from_key_path(key_path: &NSString) -> Option<Self> {
        if key_path.isEqualToString(ns_string!("URL")) {
            Some(Self::Url)
        } else if key_path.isEqualToString(ns_string!("canGoBack"))
            || key_path.isEqualToString(ns_string!("canGoForward"))
        {
            Some(Self::HistoryAvailability)
        } else {
            None
        }
    }
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
            let observation = match NavigationObservationKind::from_key_path(key_path) {
                Some(NavigationObservationKind::Url) => {
                    let current = self
                        .ivars()
                        .webview
                        .load()
                        .and_then(|webview| super::bounded_native_current_url(&webview));
                    Some(NavigationObservation::Url(current))
                }
                Some(NavigationObservationKind::HistoryAvailability) => {
                    Some(NavigationObservation::HistoryAvailability)
                }
                None => None,
            };
            if let Some(observation) = observation {
                (self.ivars().on_change)(observation);
            }
        }
    }

    unsafe impl NSObjectProtocol for NavigationObserver {}
);

impl NavigationObserver {
    pub fn install(
        mtm: MainThreadMarker,
        webview: &Retained<WKWebView>,
        on_change: impl Fn(NavigationObservation) + 'static,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn callback_key_paths_are_closed_and_keep_url_distinct_from_history_availability() {
        assert_eq!(
            NavigationObservationKind::from_key_path(ns_string!("URL")),
            Some(NavigationObservationKind::Url)
        );
        for key_path in [ns_string!("canGoBack"), ns_string!("canGoForward")] {
            assert_eq!(
                NavigationObservationKind::from_key_path(key_path),
                Some(NavigationObservationKind::HistoryAvailability)
            );
        }
        assert_eq!(
            NavigationObservationKind::from_key_path(ns_string!("title")),
            None
        );
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
