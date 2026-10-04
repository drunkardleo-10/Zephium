//! Fail-closed request-method fence for one retained Work controller.
#![deny(unsafe_op_in_unsafe_fn, clippy::undocumented_unsafe_blocks)]
use crate::platform::work_document_navigation::WorkDocumentNavigation;
use webview2_com::Microsoft::Web::WebView2::Win32::{
    ICoreWebView2, ICoreWebView2_22, COREWEBVIEW2_WEB_RESOURCE_CONTEXT_DOCUMENT,
    COREWEBVIEW2_WEB_RESOURCE_REQUEST_SOURCE_KINDS_ALL,
};
use windows_core::{Interface as _, PWSTR};
use wry::WebViewExtWindows as _;

pub(crate) struct WorkNetworkPolicy {
    core: ICoreWebView2,
    core22: ICoreWebView2_22,
    token: Option<i64>,
    filter: bool,
}
impl WorkNetworkPolicy {
    pub(crate) fn install(view: &wry::WebView, gate: WorkDocumentNavigation) -> Result<Self, ()> {
        let core = view.webview();
        let core22 = core.cast::<ICoreWebView2_22>().map_err(|_| ())?;
        // SAFETY: live STA-owned environment; the immutable response has no stream or caller-owned body.
        let blocked = unsafe {
            view.environment().CreateWebResourceResponse(
                None,
                403,
                windows_core::w!("Work request refused"),
                windows_core::w!("Cache-Control: no-store\r\nContent-Length: 0"),
            )
        }
        .map_err(|_| ())?;
        let handler = webview2_com::WebResourceRequestedEventHandler::create(Box::new(
            move |_, args| {
                let Some(args) = args else {
                    return Ok(());
                };
                // Filters are a controller-wide union: another owner may have
                // admitted fetch/XHR, scripts or images to this same event.
                // This fence owns document methods only; leave every other
                // resource unchanged for its ordinary network/content policy.
                let mut context = Default::default();
                // SAFETY: exact STA callback args and initialized typed output.
                match unsafe { args.ResourceContext(&mut context) } {
                    Ok(()) if context != COREWEBVIEW2_WEB_RESOURCE_CONTEXT_DOCUMENT => {
                        return Ok(());
                    }
                    Err(_) => {
                        // Unknown native context cannot certify this fence's
                        // exclusion; fail closed without reading page headers.
                        // SAFETY: exact callback response and immutable refusal.
                        unsafe { args.SetResponse(&blocked)? };
                        return Ok(());
                    }
                    Ok(()) => {}
                }
                let allowed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    // SAFETY: request getters return AddRef'd COM objects and allocated strings for this callback.
                    let request = unsafe { args.Request() }.ok()?;
                    let mut uri = PWSTR::null();
                    let mut method = PWSTR::null();
                    // SAFETY: initialized writable out pointers; strings are freed by the bounded adapter below.
                    unsafe {
                        request.Uri(&mut uri).ok()?;
                    }
                    let uri = super::take_pwstr_bounded(
                        uri,
                        zephium_core::blocker::MAX_NETWORK_REQUEST_URL_BYTES,
                        zephium_core::blocker::MAX_NETWORK_REQUEST_URL_BYTES,
                    )?;
                    // SAFETY: initialized writable out pointer on a live request.
                    unsafe {
                        request.Method(&mut method).ok()?;
                    }
                    let method = super::take_pwstr_bounded(
                        method,
                        zephium_core::blocker::MAX_NETWORK_REQUEST_METHOD_BYTES,
                        zephium_core::blocker::MAX_NETWORK_REQUEST_METHOD_BYTES,
                    )?;
                    // Sec-Fetch-Dest is browser-generated and forbidden to page scripts.
                    // DOCUMENT context also includes subframes; leave their ordinary page traffic intact.
                    // SAFETY: live native headers and initialized out-string, with a fixed bounded field name.
                    let headers = unsafe { request.Headers() }.ok()?;
                    let mut destination = PWSTR::null();
                    // SAFETY: native header getter on the exact event request; its string is freed by the bounded owner.
                    let main_document = if unsafe {
                        headers.GetHeader(windows_core::w!("Sec-Fetch-Dest"), &mut destination)
                    }
                    .is_ok()
                    {
                        super::take_pwstr_bounded(destination, 32, 32).and_then(|destination| {
                            match destination.as_str() {
                                "document" => Some(true),
                                "iframe" | "frame" => Some(false),
                                _ => None,
                            }
                        })
                    } else {
                        None
                    };
                    let allowed = gate.allows_windows_request(&uri, &method, main_document);
                    #[cfg(all(debug_assertions, feature = "native-agentic-work-lifetime-diagnostic"))]
                    {
                        use std::io::Write as _;
                        let method_class = match method.as_str() {
                            "POST" => "post",
                            "GET" | "HEAD" | "OPTIONS" => "safe",
                            _ => "other",
                        };
                        let _ = writeln!(std::io::stderr().lock(),
                            "windows-work-network: stage=document method={method_class} main={main_document:?} allowed={allowed} handed_on={} failed={}; content=redacted",
                            gate.handed_on(), gate.failed());
                    }
                    Some(allowed)
                }))
                .ok()
                .flatten()
                    == Some(true);
                if !allowed {
                    // SAFETY: synchronous policy refusal only strengthens any prior native/adblock response; getters and panic all refuse.
                    unsafe {
                        args.SetResponse(&blocked)?;
                    }
                }
                Ok(())
            },
        ));
        let mut owner = Self {
            core,
            core22,
            token: None,
            filter: false,
        };
        // SAFETY: fixed wildcard/context/source kind; this registration belongs to this controller only.
        unsafe {
            owner
                .core22
                .AddWebResourceRequestedFilterWithRequestSourceKinds(
                    windows_core::w!("*://*"),
                    COREWEBVIEW2_WEB_RESOURCE_CONTEXT_DOCUMENT,
                    COREWEBVIEW2_WEB_RESOURCE_REQUEST_SOURCE_KINDS_ALL,
                )
        }
        .map_err(|_| ())?;
        owner.filter = true;
        let mut token = 0;
        // SAFETY: live STA-owned handler; initialized token out pointer; native retains the handler.
        unsafe { owner.core.add_WebResourceRequested(&handler, &mut token) }.map_err(|_| ())?;
        owner.token = Some(token);
        Ok(owner)
    }
    pub(crate) fn retire(&mut self) -> bool {
        if let Some(token) = self.token {
            // SAFETY: exact registration token and controller, on their creating thread.
            if unsafe { self.core.remove_WebResourceRequested(token) }.is_err() {
                return false;
            }
            self.token = None;
        }
        if self.filter {
            // SAFETY: exact fixed filter identity installed on this controller by this owner.
            if unsafe {
                self.core22
                    .RemoveWebResourceRequestedFilterWithRequestSourceKinds(
                        windows_core::w!("*://*"),
                        COREWEBVIEW2_WEB_RESOURCE_CONTEXT_DOCUMENT,
                        COREWEBVIEW2_WEB_RESOURCE_REQUEST_SOURCE_KINDS_ALL,
                    )
            }
            .is_err()
            {
                return false;
            }
            self.filter = false;
        }
        true
    }
}
impl Drop for WorkNetworkPolicy {
    fn drop(&mut self) {
        self.retire();
    }
}
