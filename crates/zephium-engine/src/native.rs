#[cfg(target_os = "macos")]
pub(crate) fn configure(webview: &wry::WebView, radius: f64) {
    use objc2::msg_send;
    use objc2::runtime::AnyObject;
    use wry::WebViewExtMacOS;

    // NSViewWidthSizable | NSViewHeightSizable: the content fills its region and
    // follows window resize in AppKit's layout pass.
    const FLEXIBLE: usize = 2 | 16;

    let wk = webview.webview();
    unsafe {
        let _: () = msg_send![&*wk, setInspectable: true];
        let _: () = msg_send![&*wk, setTranslatesAutoresizingMaskIntoConstraints: true];
        let _: () = msg_send![&*wk, setAutoresizingMask: FLEXIBLE];
        let layer: *mut AnyObject = msg_send![&*wk, layer];
        if !layer.is_null() {
            let _: () = msg_send![layer, setCornerRadius: radius];
            let _: () = msg_send![layer, setMasksToBounds: true];
        }
    }
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn configure(_webview: &wry::WebView, _radius: f64) {}
