#[cfg(target_os = "macos")]
pub(crate) fn configure(webview: &wry::WebView, radius: f64) {
    use objc2_app_kit::{NSAutoresizingMaskOptions as Mask, NSView};
    use objc2_web_kit::WKWebView;
    use wry::WebViewExtMacOS;

    let wk = webview.webview();
    let ptr = objc2::rc::Retained::as_ptr(&wk);
    let view: &NSView = unsafe { &*ptr.cast::<NSView>() };
    let webkit: &WKWebView = unsafe { &*ptr.cast::<WKWebView>() };

    unsafe { webkit.setInspectable(true) };
    // Fill the assigned region and follow window resize in AppKit's layout pass.
    view.setTranslatesAutoresizingMaskIntoConstraints(true);
    view.setAutoresizingMask(Mask::ViewWidthSizable | Mask::ViewHeightSizable);
    if let Some(layer) = view.layer() {
        layer.setCornerRadius(radius);
        layer.setMasksToBounds(true);
    }
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn configure(_webview: &wry::WebView, _radius: f64) {}
